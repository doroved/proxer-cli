use base64::{engine::general_purpose::STANDARD as b64, Engine as _};
use cache::HOST_CACHE;
use chrono::Local;
use clap::Parser;
use hyper::client::connect::HttpConnector;
use hyper::{Body, Client, Method, Request, Response, StatusCode};
use hyper_proxy::{Intercept, Proxy as HyperProxy, ProxyConnector};
// use hyper_tls::HttpsConnector;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io;
use std::{process::Command, string::FromUtf8Error, sync::Arc, time::Duration};
use tokio::time::{sleep, Instant};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};
use tokio_native_tls::{native_tls, TlsConnector};
use toml::from_str;
use wildmatch::WildMatch;

mod cache;
mod options;

use options::Opt;

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
    pub host: String,
    pub port: u16,
    auth_credentials: AuthCredentials,
    filter: Vec<Filter>,
}

pub enum ProxyState {
    On,
    Off,
}

pub struct Proxy {
    pub interface: String,
    pub server: String,
    pub port: u16,
}

// Clients store
// enum ProxiedClient {
//     Http(Client<ProxyConnector<hyper::client::HttpConnector>>),
//     Https(Client<ProxyConnector<HttpsConnector<hyper::client::HttpConnector>>>),
// }

// Handle HTTP and HTTPS requests
pub async fn handle_request(
    req: Request<Body>,
    config: Arc<Vec<ProxyConfig>>,
) -> Result<Response<Body>, hyper::Error> {
    let addr = req.uri().authority().unwrap().to_string();
    let host = req.uri().host().unwrap();

    let options = Opt::parse();
    let time = formatted_time();

    let found_proxy = find_matching_proxy(config.as_ref(), host);

    if req.method() == Method::CONNECT {
        if let Some(_) = req.uri().authority().map(|auth| auth.to_string()) {
            tokio::spawn(async move {
                match tunnel(req, found_proxy.clone()).await {
                    Ok(_) => {}
                    Err(e) => {
                        let error_msg = e.to_string();

                        if options.log_error_all {
                            eprintln!("\x1B[31m\x1B[1m[{time}] {addr} → {error_msg}\x1B[0m");
                        } else if error_msg.contains("os error 60")
                            || error_msg.contains("Proxy connection failed")
                            || error_msg.contains("deadline has elapsed")
                        {
                            eprintln!("\x1B[31m\x1B[1m[{time}] {addr} → {error_msg}\x1B[0m");
                        }
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
        println!("HTTP request {}, req: {:?}", addr, req);
        // TODO: Proxy HTTP requests based on filters through HTTPS proxy, for HTTP proxies works.

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

        // Processing of the found proxy
        if let Some((proxy_config, _filter_name)) = found_proxy {
            match send_request_through_proxy(new_req, &proxy_config).await {
                Ok(response) => {
                    return Ok(response);
                }
                Err(err) => {
                    return Ok(Response::builder()
                        .status(StatusCode::BAD_GATEWAY)
                        .body(Body::from(format!("Error: {}", err)))
                        .unwrap());
                }
            }
        }

        // If no proxy is found, simply send the request
        let client = Client::new();
        let response = client.request(new_req).await?;
        Ok(response)
    }
}

async fn send_request_through_proxy(
    req: Request<Body>,
    proxy_config: &ProxyConfig,
) -> Result<Response<Body>, hyper::Error> {
    let mut new_req = req;
    new_req.headers_mut().insert("proxy-authorization", {
        let credentials = format!(
            "{}:{}",
            proxy_config.auth_credentials.username, proxy_config.auth_credentials.password
        );
        hyper::header::HeaderValue::from_str(&format!("Basic {}", b64.encode(credentials))).unwrap()
    });

    if proxy_config.scheme.eq_ignore_ascii_case("http") {
        // Create a new proxy connector
        let http = HttpConnector::new();

        let proxy_url = format!("http://{}:{}", proxy_config.host, proxy_config.port);
        let proxy = HyperProxy::new(
            Intercept::All,
            proxy_url.parse::<hyper::Uri>().expect("Invalid proxy URI"),
        );

        // Create proxy client
        let proxy_connector = ProxyConnector::from_proxy(http, proxy).unwrap();
        let client = Client::builder().build::<_, hyper::Body>(proxy_connector);

        // Send request
        let res = client.request(new_req).await?;

        Ok(res)
    } else {
        // Create a new proxy connector
        // let https = HttpsConnector::new();

        // let proxy_url = format!("https://{}:{}", proxy_config.host, proxy_config.port);
        // let proxy = HyperProxy::new(Intercept::All, proxy_url.parse().unwrap());

        // let proxy_connector = ProxyConnector::from_proxy(https, proxy).unwrap();

        // Create proxy client
        // let client = Client::builder().build::<_, hyper::Body>(proxy_connector);

        // Send request
        // let res = client.request(new_req).await?;
        // Ok(res)

        // return Ok(Response::builder()
        //     .status(StatusCode::OK)
        //     .body(Body::from("Proxer does not currently support HTTPS proxies for `http://` connections; please use an HTTP proxy. Support: https://t.me/macproxer"))
        //     .unwrap());

        let client = Client::new();
        let response = client.request(new_req).await?;
        Ok(response)
    }
}

async fn tunnel(
    req: Request<Body>,
    found_proxy: Option<(ProxyConfig, String)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = Opt::parse();

    let addr = req.uri().authority().unwrap().to_string();

    let time = formatted_time();

    if let Some((proxy, filter_name)) = found_proxy {
        println!(
            "\x1B[34m\x1B[1m[{time}] {addr} → {} · {} · {filter_name}\x1B[0m",
            proxy.name, proxy.scheme
        );

        let proxy_addr = format!("{}:{}", proxy.host, proxy.port);

        let tcp_stream =
            timeout(Duration::from_secs(10), TcpStream::connect(&proxy_addr)).await??;

        match proxy.scheme.as_str() {
            "HTTP" => {
                handle_http_proxy(req, tcp_stream, &addr, &proxy).await?;
            }
            "HTTPS" => {
                handle_https_proxy(req, tcp_stream, &addr, &proxy).await?;
            }
            _ => return Err(format!("Unsupported proxy scheme: {}", proxy.scheme).into()),
        }

        return Ok(());
    } else {
        println!("\x1b[1m[{time}] {addr} → Direct connection\x1b[0m");

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
            let n = client_reader.read(&mut buffer[bytes_read..]).await?;

            if n == 0 {
                return Err("Connection closed".into());
            }

            bytes_read += n;
        }

        if options.dpi {
            // Send the first byte to the server
            server.write_all(&buffer[..1]).await?;

            // Send the rest of the bytes to the server
            server.write_all(&buffer[1..]).await?;
        } else {
            // Send the all bytes to the server
            server.write_all(&buffer[..bytes_read]).await?;
        }

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

// Kill all proxer processes
pub fn terminate_proxer() {
    let _ = Command::new("sh")
        .args(&["-c", "kill $(pgrep proxer)"])
        .output()
        .expect("Failed to execute `kill $(pgrep proxer)` command to terminate proxer processes");
}

// Check if the host is allowed by the allowed_hosts list
fn is_host_allowed(req_host: &str, allowed_hosts: &[String]) -> bool {
    for allowed_host in allowed_hosts {
        if WildMatch::new(allowed_host).matches(req_host) {
            return true;
        }
    }
    false
}

// Implement a function to load the configuration from a JSON file and search for a matching proxy
pub fn find_matching_proxy(
    config_file: &[ProxyConfig],
    req_host: &str,
) -> Option<(ProxyConfig, String)> {
    let mut host_cache = HOST_CACHE.lock().unwrap();
    // println!("{:?}", host_cache);

    // Check host in cache
    if host_cache.contains(req_host) {
        if let Some((config_index, filter_index)) = host_cache.get(req_host) {
            let proxy_config = config_file[*config_index].clone();
            let filter_name = proxy_config.filter[*filter_index].name.clone();

            return Some((proxy_config, filter_name));
        }
    }

    for (config_index, proxy_config) in config_file.iter().enumerate() {
        // Skip disabled proxies
        if !proxy_config.enabled {
            continue;
        }

        // Find by filters
        for (filter_index, filter) in proxy_config.filter.iter().enumerate() {
            if is_host_allowed(req_host, &filter.domains) {
                // Add host to cache
                host_cache.add(req_host.to_string(), config_index, filter_index);
                return Some((proxy_config.clone(), filter.name.clone()));
            }
        }
    }

    None
}

fn formatted_time() -> String {
    let now = Local::now();
    now.format("%H:%M:%S").to_string()
}

async fn handle_http_proxy(
    req: Request<Body>,
    mut tcp_stream: TcpStream,
    addr: &str,
    proxy: &ProxyConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    send_connect_request(req, &mut tcp_stream, addr, &proxy).await?;
    Ok(())
}

async fn handle_https_proxy(
    req: Request<Body>,
    tcp_stream: TcpStream,
    addr: &str,
    proxy: &ProxyConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let connector = TlsConnector::from(native_tls::TlsConnector::new()?);
    let mut tls_stream = timeout(
        Duration::from_secs(10),
        connector.connect(&proxy.host, tcp_stream),
    )
    .await??;

    send_connect_request(req, &mut tls_stream, addr, &proxy).await?;
    Ok(())
}

async fn send_connect_request<T>(
    req: Request<Body>,
    stream: &mut T,
    addr: &str,
    proxy: &ProxyConfig,
) -> Result<(), Box<dyn std::error::Error>>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let options = Opt::parse();
    let package = package_info();

    let mut connect_req = format!(
        "CONNECT {addr} HTTP/1.1\r\n\
        Host: {addr}\r\n\
        Proxy-Connection: Keep-Alive\r\n\
        User-Agent: {}/{}\r\n",
        package.name, package.version
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
            let command = format!("-set{proxy_type}");

            let _ = self
                .execute_command(&[
                    &command,
                    &self.interface,
                    &self.server,
                    &self.port.to_string(),
                ])
                .expect(&format!("Failed to set {proxy_type}"));
        }
    }

    pub fn set_state(&self, state: ProxyState) {
        let proxy_types = self.get_proxy_types();
        let proxy_state = match state {
            ProxyState::On => "on",
            ProxyState::Off => "off",
        };

        for proxy_type in proxy_types.iter() {
            let command = format!("-set{proxy_type}state");

            let _ = self
                .execute_command(&[&command, &self.interface, proxy_state])
                .expect(&format!("Failed to set {proxy_type} state"));
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

pub fn to_sha256(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input);

    let result = hasher.finalize();
    format!("{:x}", result)
}

// pub async fn measure_ping(server_addr: &str) -> Result<Duration, std::io::Error> {
//     let start = Instant::now();

//     match TcpStream::connect(server_addr).await {
//         Ok(_) => Ok(start.elapsed()),
//         Err(e) => Err(e),
//     }
// }

// pub async fn ping_loop(server_addr: &str) {
//     loop {
//         match measure_ping(server_addr).await {
//             Ok(duration) => println!("{server_addr} | Ping: {} ms", duration.as_millis()),
//             Err(e) => eprintln!("Error connecting to server: {}", e),
//         }

//         sleep(Duration::from_secs(5)).await;
//     }
// }

pub async fn measure_ping(proxy_config: &[ProxyConfig]) -> Result<Duration, std::io::Error> {
    let start = Instant::now();

    for proxy in proxy_config {
        if proxy.enabled {
            let address = format!("{}:{}", proxy.host, proxy.port);

            match TcpStream::connect(&address).await {
                Ok(_) => {
                    println!("{address} | Ping: {} ms", start.elapsed().as_millis());
                    return Ok(start.elapsed());
                }
                Err(e) => {
                    eprintln!("Error connecting to {address}: {e}");
                    return Err(e);
                }
            }
        }
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "Unable to connect to any proxy server",
    ))
}

pub async fn ping_loop(proxy_config: &[ProxyConfig]) {
    loop {
        let _ = measure_ping(proxy_config).await;
        // match measure_ping(proxy_config).await {
        //     Ok(duration) => println!("{server_addr} | Ping: {} ms", duration.as_millis()),
        //     Err(e) => eprintln!("Error connecting to server: {}", e),
        // }

        sleep(Duration::from_secs(5)).await;
    }
}
