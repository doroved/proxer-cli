use std::{pin::Pin, time::Duration};

use super::ProxyConfig;
use crate::{options::Opt, server::utils::to_sha256};

use base64::{engine::general_purpose::STANDARD as b64, Engine as _};
use clap::Parser;

use hyper::upgrade::Upgraded;
use hyper_util::rt::TokioIo;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};
use tokio_native_tls::{native_tls, TlsConnector};

trait AsyncReadWrite: AsyncRead + AsyncWrite + Send {}
impl<T: AsyncRead + AsyncWrite + Send> AsyncReadWrite for T {}

const APP_NAME: &str = env!("CARGO_PKG_NAME");
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub async fn tunnel_direct(
    upgraded: Upgraded,
    addr: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("{addr} → DIRECT connection");

    let mut server = timeout(Duration::from_secs(30), TcpStream::connect(&addr)).await??;

    timeout(
        Duration::from_secs(30),
        tokio::io::copy_bidirectional(&mut TokioIo::new(upgraded), &mut server),
    )
    .await??;

    Ok(())
}

pub async fn tunnel_via_proxy(
    upgraded: Upgraded,
    addr: &str,
    proxy: ProxyConfig,
    filter_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!(
        "\x1B[34m{addr} → {} · {} · {filter_name}\x1B[0m",
        proxy.name,
        proxy.scheme
    );

    let options = Opt::parse();

    let proxy_addr = format!("{}:{}", &proxy.host, &proxy.port);
    let proxy_host = proxy_addr.split(':').next().unwrap();

    let tcp = timeout(Duration::from_secs(30), TcpStream::connect(&proxy_addr)).await??;

    let mut stream: Pin<Box<dyn AsyncReadWrite>> = if proxy.scheme.eq_ignore_ascii_case("http") {
        Box::pin(tcp)
    } else {
        let tls = TlsConnector::from(native_tls::TlsConnector::new()?);
        Box::pin(timeout(Duration::from_secs(30), tls.connect(proxy_host, tcp)).await??)
    };

    let mut connect_req = format!(
        "CONNECT {addr} HTTP/1.1\r\n\
        Host: {addr}\r\n\
        Proxy-Connection: Keep-Alive\r\n\
        User-Agent: {}/{}\r\n",
        APP_NAME, APP_VERSION
    );

    // Get the auth credentials from the proxy config
    let auth_username = proxy.auth_credentials.username.clone();
    let auth_password = proxy.auth_credentials.password.clone();

    // Add auth credentials to the request if they are provided
    if !auth_username.is_empty() && !auth_password.is_empty() {
        connect_req.push_str(&format!(
            "Proxy-Authorization: Basic {auth_credentials}\r\n",
            auth_credentials = b64.encode(format!("{}:{}", auth_username, auth_password))
        ));
    }

    // Add token to the request if it is provided
    if let Some(token) = options.token.as_ref() {
        if proxy.scheme.eq_ignore_ascii_case("http") {
            connect_req.push_str(&format!("x-http-secret-token: {}\r\n", to_sha256(token)));
        } else if proxy.scheme.eq_ignore_ascii_case("https") {
            connect_req.push_str(&format!("x-https-secret-token: {}\r\n", to_sha256(token)));
        }
    }

    connect_req.push_str("\r\n");
    stream.write_all(connect_req.as_bytes()).await?;

    let mut response = [0u8; 1024];
    let n = timeout(Duration::from_secs(5), stream.read(&mut response)).await??;
    if !response[..n].starts_with(b"HTTP/1.1 200") {
        return Err(format!("{:?}", String::from_utf8_lossy(&response[..n])).into());
    }

    let (from_client, from_server) = timeout(
        Duration::from_secs(30),
        tokio::io::copy_bidirectional(&mut TokioIo::new(upgraded), &mut stream),
    )
    .await??;

    tracing::info!(
        "{addr} → Client wrote {:.2} KB and received {:.2} KB",
        from_client as f64 / 1024.0,
        from_server as f64 / 1024.0
    );

    stream.shutdown().await?;

    Ok(())
}
