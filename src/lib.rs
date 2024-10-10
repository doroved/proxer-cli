use base64::{engine::general_purpose::STANDARD as b64, Engine as _};
use chrono::Local;
use hyper::{Body, Client, Method, Request, Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::{process::Command, string::FromUtf8Error, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};
use tokio_native_tls::{native_tls, TlsConnector};
use toml::from_str;
use wildmatch::WildMatch;

// Struct for storing package information
#[derive(Debug, Deserialize)]
pub struct CargoToml {
    pub package: Package,
}

#[derive(Debug, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
}

// Struct for storing proxy configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
struct AuthCredentials {
    username: String,
    password: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Filter {
    name: String,
    domains: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProxyConfig {
    name: String,
    enabled: bool,
    scheme: String,
    host: String,
    port: u16,
    auth_credentials: AuthCredentials,
    filter: Vec<Filter>,
}

pub struct Proxy {
    pub interface: String,
    pub server: String,
    pub port: u16,
}

// Handle HTTP and HTTPS requests
pub async fn handle_request(
    req: Request<Body>,
    config: Arc<Vec<ProxyConfig>>,
) -> Result<Response<Body>, hyper::Error> {
    let addr = req.uri().authority().unwrap().to_string();
    let time = formatted_time();

    if req.method() == Method::CONNECT {
        if let Some(_) = req.uri().authority().map(|auth| auth.to_string()) {
            let config_clone = Arc::clone(&config);

            tokio::spawn(async move {
                match tunnel(req, config_clone).await {
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("\x1B[31m\x1B[1m[{time}] {} -> {}\x1B[0m", addr, e);
                    }
                }
            });

            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(Body::empty())
                .unwrap())
        } else {
            Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("Invalid CONNECT request"))
                .unwrap())
        }
    } else {
        println!("HTTP request {}", req.uri().to_string());
        // TODO: Proxy HTTP requests based on allow_hosts

        // Create client for HTTP request
        let client = Client::new();

        // Copy the necessary data from req
        let method = req.method().clone();
        let uri = req.uri().clone();
        let headers = req.headers().clone();
        let body = req.into_body();

        // Create a new request
        let mut new_req = match Request::builder().method(method).uri(uri).body(body) {
            Ok(req) => req,
            Err(e) => {
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Body::from(format!("Error creating request: {}", e)))
                    .unwrap())
            }
        };

        // Copy headers
        new_req.headers_mut().extend(headers);

        // Send the request and return the response
        client.request(new_req).await
    }
}

async fn tunnel(
    req: Request<Body>,
    config: Arc<Vec<ProxyConfig>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let addr = req.uri().authority().unwrap().to_string();
    let host = req.uri().host().unwrap();

    let time = formatted_time();

    match find_matching_proxy(config.as_ref(), host) {
        Some(proxy) => {
            println!(
                "\x1B[34m\x1B[1m[{time}] {} -> {} · {}\x1B[0m",
                addr, proxy.name, proxy.scheme
            );

            let proxy_addr = format!("{}:{}", proxy.host, proxy.port);
            let proxy_user = proxy.auth_credentials.username;
            let proxy_pass = proxy.auth_credentials.password;

            let tcp_stream =
                timeout(Duration::from_secs(10), TcpStream::connect(&proxy_addr)).await??;

            match proxy.scheme.as_str() {
                "HTTP" => {
                    handle_http_proxy(req, tcp_stream, &addr, &proxy_user, &proxy_pass).await?;
                }
                "HTTPS" => {
                    handle_https_proxy(
                        req,
                        tcp_stream,
                        &addr,
                        &proxy.host,
                        &proxy_user,
                        &proxy_pass,
                    )
                    .await?;
                }
                _ => return Err(format!("Unsupported proxy scheme: {}", proxy.scheme).into()),
            }

            return Ok(());
        }
        None => {
            println!("[{time}] {} -> Direct connection", addr);

            // Connect to the server
            let mut server = timeout(Duration::from_secs(10), TcpStream::connect(&addr)).await??;

            // Get the upgraded connection from the client
            let upgraded = hyper::upgrade::on(req).await?;
            let (mut client_reader, mut client_writer) = tokio::io::split(upgraded);

            // Buffer for reading the first bytes of Client Hello
            let mut buffer = [0u8; 1024];
            let mut bytes_read = 0;

            // Read the first bytes of Client Hello
            while bytes_read < 5 {
                // Minimum length of TLS record
                let n = client_reader.read(&mut buffer[bytes_read..]).await?;
                if n == 0 {
                    return Err("Connection closed".into());
                }
                bytes_read += n;
            }

            // Send the first byte to the server
            server.write_all(&buffer[..1]).await?;

            // Send the rest of the bytes to the server
            server.write_all(&buffer[1..bytes_read]).await?;

            // Split the server connection into reader and writer
            let (mut server_reader, mut server_writer) = server.split();

            // Copy data from client to server
            let client_to_server = async {
                tokio::io::copy(&mut client_reader, &mut server_writer).await?;
                server_writer.shutdown().await
            };

            // Copy data from server to client
            let server_to_client = async {
                tokio::io::copy(&mut server_reader, &mut client_writer).await?;
                client_writer.shutdown().await
            };

            // Wait for both tasks to complete
            tokio::try_join!(client_to_server, server_to_client)?;

            return Ok(());
        }
    };
}

pub fn get_default_interface() -> String {
    // Use networksetup to get the default interface
    let output = Command::new("bash")
        .arg("-c")
        .arg("networksetup -listnetworkserviceorder | grep `(route -n get default | grep 'interface' || route -n get -inet6 default | grep 'interface') | cut -d ':' -f2` -B 1 | head -n 1 | cut -d ' ' -f 2-")
        .output()
        .expect("Failed to get default interface");

    // Convert the result to a string
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn package_info() -> Package {
    let content = include_str!("../Cargo.toml");

    // Parse the content of the file
    let cargo: CargoToml = from_str(&content).expect("Error parsing Cargo.toml");

    // Return the package information
    cargo.package
}

pub fn terminate_proxer() {
    let _ = Command::new("sh")
        .args(&["-c", "kill $(pgrep proxer)"])
        .output()
        .expect("Failed to execute `kill $(pgrep proxer)` command to terminate proxer processes");
}

fn is_host_allowed(req_host: &str, allowed_hosts: &[String]) -> bool {
    for allowed_host in allowed_hosts {
        if WildMatch::new(allowed_host).matches(req_host) {
            return true;
        }
    }
    false
}

// Implement a function to load the configuration from a JSON file and search for a matching proxy
pub fn find_matching_proxy(config_file: &[ProxyConfig], req_host: &str) -> Option<ProxyConfig> {
    // Read the contents of the file proxer.json5
    for config in config_file {
        // Skip disabled proxies
        if !config.enabled {
            continue;
        }

        // Find by filters
        for filter in &config.filter {
            if is_host_allowed(req_host, &filter.domains) {
                return Some(config.clone());
            }
        }
    }

    None
}

fn formatted_time() -> String {
    let now = Local::now();
    now.format("%H:%M:%S").to_string()
    // now.format("%Y-%m-%d %H:%M:%S").to_string()
}

async fn handle_http_proxy(
    req: Request<Body>,
    mut tcp_stream: TcpStream,
    addr: &str,
    proxy_user: &str,
    proxy_pass: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    send_connect_request(req, &mut tcp_stream, addr, proxy_user, proxy_pass).await?;
    Ok(())
}

async fn handle_https_proxy(
    req: Request<Body>,
    tcp_stream: TcpStream,
    addr: &str,
    proxy_host: &str,
    proxy_user: &str,
    proxy_pass: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let connector = TlsConnector::from(native_tls::TlsConnector::new()?);
    let mut tls_stream = timeout(
        Duration::from_secs(10),
        connector.connect(proxy_host, tcp_stream),
    )
    .await??;

    send_connect_request(req, &mut tls_stream, addr, proxy_user, proxy_pass).await?;
    Ok(())
}

async fn send_connect_request<T>(
    req: Request<Body>,
    stream: &mut T,
    addr: &str,
    proxy_user: &str,
    proxy_pass: &str,
) -> Result<(), Box<dyn std::error::Error>>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let package = package_info();
    let auth = b64.encode(format!("{}:{}", proxy_user, proxy_pass));
    let connect_req = format!(
        "CONNECT {} HTTP/1.1\r\n\
        Host: {}\r\n\
        Proxy-Connection: Keep-Alive\r\n\
        Proxy-Authorization: Basic {}\r\n\
        User-Agent: {}/{}\r\n\
        \r\n",
        addr, addr, auth, package.name, package.version
    );

    stream.write_all(connect_req.as_bytes()).await?;

    let mut response = [0u8; 1024];
    let n = timeout(Duration::from_secs(5), stream.read(&mut response)).await??;

    if !response[..n].windows(3).any(|window| window == b"200") {
        return Err(format!(
            "Proxy connection failed: {:?}",
            String::from_utf8_lossy(&response[..n])
        )
        .into());
    }

    let upgraded = hyper::upgrade::on(req).await?;

    let (mut proxy_reader, mut proxy_writer) = tokio::io::split(stream);
    let (mut client_reader, mut client_writer) = tokio::io::split(upgraded);

    let client_to_server = tokio::io::copy(&mut client_reader, &mut proxy_writer);
    let server_to_client = tokio::io::copy(&mut proxy_reader, &mut client_writer);

    tokio::try_join!(client_to_server, server_to_client)?;

    Ok(())
}

impl Proxy {
    pub fn init(interface: String, server: &str, port: u16) -> Self {
        Proxy {
            interface,
            server: String::from(server),
            port,
        }
    }

    pub fn set(&self) {
        // Define proxy types
        let proxy_types = self.get_proxy_types();

        // Go through each proxy type and set server and port
        for proxy_type in proxy_types.iter() {
            let command = format!("-set{}", proxy_type);

            let _ = self
                .execute_command(&[
                    &command,
                    &self.interface,
                    &self.server,
                    &self.port.to_string(),
                ])
                .expect(&format!("Failed to set {}", proxy_type));
        }
    }

    pub fn set_state(&self, state: &str) {
        let proxy_types = self.get_proxy_types();

        for proxy_type in proxy_types.iter() {
            let command = format!("-set{}state", proxy_type);

            let _ = self
                .execute_command(&[&command, &self.interface, state])
                .expect(&format!("Failed to set {} state", proxy_type));
        }
    }

    fn get_proxy_types(&self) -> [&'static str; 2] {
        ["webproxy", "securewebproxy"]
    }

    fn execute_command(&self, args: &[&str]) -> Result<String, FromUtf8Error> {
        let output = Command::new("networksetup")
            .args(args)
            .output()
            .expect("Failed to execute command");

        String::from_utf8(output.stdout)
    }
}
