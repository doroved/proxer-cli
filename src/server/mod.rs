use crate::options::Opt;
mod system_proxy;

mod proxy;
mod tunnel;
mod utils;

use std::sync::Arc;
use std::{fs, net::SocketAddr};

use clap::Parser;
use hyper::{body, server::conn::http1, service::service_fn, Request};
use hyper_util::rt::TokioIo;

use serde::{Deserialize, Serialize};
use system_proxy::{ProxyState, SystemProxy};
use tokio::net::TcpListener;
use utils::terminate_proxer;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthCredentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Filter {
    pub name: String,
    pub domains: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProxyConfig {
    pub name: String,
    pub enabled: bool,
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub auth_credentials: AuthCredentials,
    pub filter: Vec<Filter>,
}

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Close all proxer-cli processes
    terminate_proxer();

    // Parse command-line options
    let options = Opt::parse();

    // Read the config file or use the default one
    let config_path = options.config.unwrap_or_else(|| {
        tracing::info!("Using default config file ~/.proxer-cli/config.json5");
        let home_dir = std::env::var("HOME").expect("$HOME environment variable not set");
        format!("{home_dir}/.proxer-cli/config.json5")
    });

    let config = fs::read_to_string(&config_path)
        .unwrap_or_else(|_| panic!("Failed to read config file: {config_path}"));
    let parsed_config: Vec<ProxyConfig> = json5::from_str(&config)
        .unwrap_or_else(|_| panic!("Failed to parse config file: {config_path}"));
    let proxy_config_arc = Arc::new(parsed_config);

    let port = options.port.unwrap_or(5555);

    let system_proxy = SystemProxy::init(port);
    system_proxy.set();
    system_proxy.set_state(ProxyState::On);

    tokio::spawn(async move {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!("Failed to install Control+C handler: {}", err);
            return;
        }

        tracing::info!("Stopping proxy server");
        system_proxy.set_state(ProxyState::Off);
        std::process::exit(0);
    });

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("Listening on {}", addr);

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let proxy_config = Arc::clone(&proxy_config_arc);

        let service = service_fn(move |req: Request<body::Incoming>| {
            let proxy_config = Arc::clone(&proxy_config);
            async move { proxy::handle_request(req, proxy_config).await }
        });

        tokio::spawn(async move {
            if let Err(err) = http1::Builder::new()
                .preserve_header_case(true)
                .title_case_headers(true)
                .serve_connection(io, service)
                .with_upgrades()
                .await
            {
                tracing::error!("Server error: {}", err);
            }
        });
    }
}
