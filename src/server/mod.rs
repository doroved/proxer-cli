use crate::options::Opt;
mod system_proxy;

mod proxy;
mod tunnel;
mod utils;

use std::net::Ipv4Addr;
use std::sync::Arc;
use std::{fs, net::SocketAddr};

use clap::Parser;
use hyper::{body, server::conn::http1, service::service_fn, Request};
use hyper_util::rt::TokioIo;

use serde::{Deserialize, Serialize};
use system_proxy::{ProxyState, SystemProxy};
use tokio::net::TcpListener;
use utils::{terminate_proxer, tracing_error};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Auth {
    pub credentials: AuthCredentials,
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthCredentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Rules {
    pub name: String,
    pub hosts: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProxyConfig {
    pub name: String,
    pub enabled: bool,
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub auth: Auth,
    pub rules: Vec<Rules>,
}

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Close all proxer-cli processes
    terminate_proxer();

    // Parse command-line options
    let options = Opt::parse();

    tracing::info!(
        "Default connection limit (soft, hard): {:?}",
        rlimit::Resource::NOFILE.get()?
    );

    // Set max open connection limit
    match rlimit::increase_nofile_limit(rlimit::INFINITY) {
        Ok(limit) => {
            tracing::info!("Setting max open connection limit to {}", limit);
        }
        Err(e) => {
            tracing::error!("\x1B[31mFailed to increase the open connection limit: {e}\x1B[0m");
            std::process::exit(1);
        }
    }

    // Read the config file or use the default one
    let config_path = options.config.unwrap_or_else(|| {
        tracing::info!("Using default config file ~/.proxer-cli/config.json");
        let home_dir = std::env::var("HOME").expect("$HOME environment variable not set");
        format!("{home_dir}/.proxer-cli/config.json")
    });

    let config = fs::read_to_string(&config_path)?;
    let parsed_config: Vec<ProxyConfig> = serde_json::from_str(&config)?;
    let proxy_config = Arc::new(parsed_config);

    let port = options.port.unwrap_or(5555);

    let system_proxy = SystemProxy::init(port);
    system_proxy.set();
    system_proxy.set_state(ProxyState::On);

    tokio::spawn(async move {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!("Failed to install Control+C handler: {}", err);
            return;
        }

        tracing::info!("Stopping proxer-cli");
        system_proxy.set_state(ProxyState::Off);
        std::process::exit(0);
    });

    let addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port);
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("Listening on {}", addr);

    loop {
        let (stream, _) = listener.accept().await?;
        let proxy_config = Arc::clone(&proxy_config);

        let service = service_fn(move |req: Request<body::Incoming>| {
            let proxy_config = Arc::clone(&proxy_config);
            async move { proxy::handle_request(req, proxy_config).await }
        });

        tokio::spawn(async move {
            if let Err(err) = http1::Builder::new()
                .preserve_header_case(true)
                .title_case_headers(true)
                .serve_connection(TokioIo::new(stream), service)
                .with_upgrades()
                .await
            {
                tracing_error(&format!("Server error: {}", err));
            }
        });
    }
}
