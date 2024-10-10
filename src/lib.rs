use base64::{engine::general_purpose::STANDARD as b64, Engine as _};
use chrono::Local;
use hyper::{Body, Client, Method, Request, Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::{process::Command, string::FromUtf8Error, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    time::{sleep, timeout},
};
use tokio_native_tls::{native_tls, TlsConnector};
use toml::from_str;
use wildmatch::WildMatch;

use libc::{c_int, setsockopt, IPPROTO_IP, IP_TTL};
use rand::{rngs::OsRng, Rng};
use std::os::unix::io::AsRawFd;

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

            // Send fake Client Hello
            println!("Sending fake Client Hello with TTL {}", 55);
            send_fake_client_hello(&mut server, "www.w3.org", 50).await?;
            println!("Fake Client Hello sent");
            sleep(Duration::from_millis(500)).await; // Добавьте эту строку
            println!("Sending real Client Hello");

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

    if !response[..n].starts_with(b"HTTP/1.1 200") {
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

//

// async fn send_fake_client_hello<T: AsyncWrite + AsRawFd + Unpin>(
//     stream: &mut T,
//     sni: &str,
//     ttl: u8,
// ) -> Result<(), Box<dyn std::error::Error>> {
//     // Set TTL for the fake packet
//     let fd = stream.as_raw_fd();
//     let ttl = ttl as c_int;
//     unsafe {
//         if setsockopt(
//             fd,
//             IPPROTO_IP,
//             IP_TTL,
//             &ttl as *const _ as *const _,
//             std::mem::size_of_val(&ttl) as u32,
//         ) != 0
//         {
//             return Err("Failed to set TTL".into());
//         }
//     }

//     let mut rng = OsRng;
//     let client_random: [u8; 32] = rng.gen();

//     let mut hello = vec![
//         0x16, 0x03, 0x01, 0x00, 0x00, // TLS record header (length will be fixed later)
//         0x01, 0x00, 0x00, 0x00, // Handshake header (length will be fixed later)
//         0x03, 0x03, // TLS version (TLS 1.2)
//     ];
//     hello.extend_from_slice(&client_random);
//     hello.push(0x00); // Session ID length
//     hello.extend_from_slice(&[0x00, 0x02]); // Cipher suites length
//     hello.extend_from_slice(&[0x13, 0x01]); // TLS_AES_128_GCM_SHA256
//     hello.push(0x01); // Compression methods length
//     hello.push(0x00); // null compression

//     // Extensions
//     let mut extensions = vec![];

//     // SNI extension
//     let mut sni_extension = vec![
//         0x00, 0x00, // Extension type: server_name
//         0x00, 0x00, // Extension length (will be fixed later)
//         0x00, 0x00, // Server Name list length (will be fixed later)
//         0x00, // Server Name Type: host_name
//     ];
//     sni_extension.extend_from_slice(&(sni.len() as u16).to_be_bytes());
//     sni_extension.extend_from_slice(sni.as_bytes());

//     let sni_extension_length = (sni_extension.len() - 4) as u16;
//     sni_extension[2..4].copy_from_slice(&sni_extension_length.to_be_bytes());
//     sni_extension[4..6].copy_from_slice(&(sni_extension_length - 2).to_be_bytes());

//     extensions.extend_from_slice(&sni_extension);

//     // Add extensions length
//     let extensions_length = extensions.len() as u16;
//     hello.extend_from_slice(&extensions_length.to_be_bytes());
//     hello.extend_from_slice(&extensions);

//     // Fix record and handshake lengths
//     let handshake_length = (hello.len() - 9) as u16;
//     let record_length = (hello.len() - 5) as u16;
//     hello[3..5].copy_from_slice(&record_length.to_be_bytes());
//     hello[6..9].copy_from_slice(&handshake_length.to_be_bytes());

//     // Send the fake Client Hello
//     stream.write_all(&hello).await?;

//     // Reset TTL to default value (64 is a common default)
//     let default_ttl = 64 as c_int;
//     unsafe {
//         if setsockopt(
//             fd,
//             IPPROTO_IP,
//             IP_TTL,
//             &default_ttl as *const _ as *const _,
//             std::mem::size_of_val(&default_ttl) as u32,
//         ) != 0
//         {
//             return Err("Failed to reset TTL".into());
//         }
//     }

//     Ok(())
// }

// Этот вариант не вызывает паники
// async fn send_fake_client_hello<T: AsyncWrite + AsRawFd + Unpin>(
//     stream: &mut T,
//     sni: &str,
//     ttl: u8,
// ) -> Result<(), Box<dyn std::error::Error>> {
//     // Set TTL for the fake packet
//     let fd = stream.as_raw_fd();
//     let ttl = ttl as c_int;
//     unsafe {
//         if setsockopt(
//             fd,
//             IPPROTO_IP,
//             IP_TTL,
//             &ttl as *const _ as *const _,
//             std::mem::size_of_val(&ttl) as u32,
//         ) != 0
//         {
//             return Err("Failed to set TTL".into());
//         }
//     }

//     let mut rng = OsRng;
//     let client_random: [u8; 32] = rng.gen();

//     let mut hello = vec![
//         0x16, 0x03, 0x01, 0x00, 0x00, // TLS record header (length will be fixed later)
//         0x01, 0x00, 0x00, 0x00, // Handshake header (length will be fixed later)
//         0x03, 0x03, // TLS version (TLS 1.2)
//     ];
//     hello.extend_from_slice(&client_random);
//     hello.push(0x00); // Session ID length
//     hello.extend_from_slice(&[0x00, 0x02]); // Cipher suites length
//     hello.extend_from_slice(&[0x13, 0x01]); // TLS_AES_128_GCM_SHA256
//     hello.push(0x01); // Compression methods length
//     hello.push(0x00); // null compression

//     // Extensions
//     let mut extensions = vec![];

//     // SNI extension
//     let mut sni_extension = vec![
//         0x00, 0x00, // Extension type: server_name
//         0x00, 0x00, // Extension length (will be fixed later)
//         0x00, 0x00, // Server Name list length (will be fixed later)
//         0x00, // Server Name Type: host_name
//     ];
//     sni_extension.extend_from_slice(&(sni.len() as u16).to_be_bytes());
//     sni_extension.extend_from_slice(sni.as_bytes());

//     let sni_extension_length = (sni_extension.len() - 4) as u16;
//     sni_extension[2..4].copy_from_slice(&sni_extension_length.to_be_bytes());
//     sni_extension[4..6].copy_from_slice(&(sni_extension_length - 2).to_be_bytes());

//     extensions.extend_from_slice(&sni_extension);

//     // Add extensions length
//     let extensions_length = extensions.len() as u16;
//     hello.extend_from_slice(&extensions_length.to_be_bytes());
//     hello.extend_from_slice(&extensions);

//     // Fix record and handshake lengths
//     let handshake_length = (hello.len() - 9) as u32;
//     let record_length = (hello.len() - 5) as u16;
//     hello[3..5].copy_from_slice(&record_length.to_be_bytes());

//     // Create a 3-byte array for handshake length
//     let handshake_length_bytes = [
//         (handshake_length >> 16) as u8,
//         (handshake_length >> 8) as u8,
//         handshake_length as u8,
//     ];
//     hello[6..9].copy_from_slice(&handshake_length_bytes);

//     // Send the fake Client Hello
//     stream.write_all(&hello).await?;

//     // Reset TTL to default value (64 is a common default)
//     let default_ttl = 64 as c_int;
//     unsafe {
//         if setsockopt(
//             fd,
//             IPPROTO_IP,
//             IP_TTL,
//             &default_ttl as *const _ as *const _,
//             std::mem::size_of_val(&default_ttl) as u32,
//         ) != 0
//         {
//             return Err("Failed to reset TTL".into());
//         }
//     }

//     Ok(())
// }

// async fn send_fake_client_hello<T: AsyncWrite + AsRawFd + Unpin>(
//     stream: &mut T,
//     sni: &str,
//     ttl: u8,
// ) -> Result<(), Box<dyn std::error::Error>> {
//     // Set TTL for the fake packet
//     let fd = stream.as_raw_fd();
//     let ttl = ttl as c_int;
//     unsafe {
//         if setsockopt(
//             fd,
//             IPPROTO_IP,
//             IP_TTL,
//             &ttl as *const _ as *const _,
//             std::mem::size_of_val(&ttl) as u32,
//         ) != 0
//         {
//             return Err("Failed to set TTL".into());
//         }
//     }

//     let mut rng = OsRng;
//     let client_random: [u8; 32] = rng.gen();

//     let mut hello = vec![
//         0x16, // Content Type: Handshake
//         0x03, 0x01, // Version: TLS 1.0 (for maximum compatibility)
//         0x00, 0x00, // Length (to be filled later)
//     ];

//     let mut handshake = vec![
//         0x01, // Handshake Type: Client Hello
//         0x00, 0x00, 0x00, // Length (to be filled later)
//         0x03, 0x03, // Version: TLS 1.2
//     ];
//     handshake.extend_from_slice(&client_random);

//     // Session ID (32 bytes, random)
//     handshake.push(32);
//     handshake.extend_from_slice(&rng.gen::<[u8; 32]>());

//     // Cipher Suites
//     let cipher_suites = [
//         0xea, 0xea, 0x13, 0x01, 0x13, 0x02, 0x13, 0x03, 0xc0, 0x2b, 0xc0, 0x2f, 0xc0, 0x2c, 0xc0,
//         0x30, 0xcc, 0xa9, 0xcc, 0xa8, 0xc0, 0x13, 0xc0, 0x14, 0x00, 0x9c, 0x00, 0x9d, 0x00, 0x2f,
//         0x00, 0x35,
//     ];
//     handshake.extend_from_slice(&(cipher_suites.len() as u16).to_be_bytes());
//     handshake.extend_from_slice(&cipher_suites);

//     // Compression Methods
//     handshake.extend_from_slice(&[0x01, 0x00]);

//     // Extensions
//     let mut extensions = vec![];

//     // Extension: server_name
//     let mut sni_extension = vec![
//         0x00, 0x00, // Extension Type: server_name
//         0x00, 0x00, // Length (to be filled later)
//         0x00, 0x00, // Server Name list length (to be filled later)
//         0x00, // Server Name Type: host_name
//     ];
//     sni_extension.extend_from_slice(&(sni.len() as u16).to_be_bytes());
//     sni_extension.extend_from_slice(sni.as_bytes());
//     let sni_extension_length = (sni_extension.len() - 4) as u16;
//     sni_extension[2..4].copy_from_slice(&sni_extension_length.to_be_bytes());
//     sni_extension[4..6].copy_from_slice(&(sni_extension_length - 2).to_be_bytes());
//     extensions.extend_from_slice(&sni_extension);

//     // Extension: supported_versions
//     extensions.extend_from_slice(&[
//         0x00, 0x2b, // Extension Type: supported_versions
//         0x00, 0x07, // Length
//         0x06, // Supported Versions length
//         0x1a, 0x1a, // Reserved (GREASE)
//         0x03, 0x04, // TLS 1.3
//         0x03, 0x03, // TLS 1.2
//     ]);

//     // Extension: signature_algorithms
//     extensions.extend_from_slice(&[
//         0x00, 0x0d, // Extension Type: signature_algorithms
//         0x00, 0x12, // Length
//         0x00, 0x10, // Signature Algorithms Length
//         0x04, 0x03, 0x08, 0x04, 0x04, 0x01, 0x05, 0x03, 0x08, 0x05, 0x05, 0x01, 0x08, 0x06, 0x06,
//         0x01,
//     ]);

//     // Extension: supported_groups
//     extensions.extend_from_slice(&[
//         0x00, 0x0a, // Extension Type: supported_groups
//         0x00, 0x0c, // Length
//         0x00, 0x0a, // Supported Groups List Length
//         0x8a, 0x8a, // Reserved (GREASE)
//         0x63, 0x99, // X25519Kyber768Draft00
//         0x00, 0x1d, // x25519
//         0x00, 0x17, // secp256r1
//         0x00, 0x18, // secp384r1
//     ]);

//     // Add extensions length
//     let extensions_length = extensions.len() as u16;
//     handshake.extend_from_slice(&extensions_length.to_be_bytes());
//     handshake.extend_from_slice(&extensions);

//     // Update handshake length
//     let handshake_length = (handshake.len() - 4) as u32;
//     handshake[1..4].copy_from_slice(&handshake_length.to_be_bytes()[1..]);

//     // Add handshake to hello
//     hello.extend_from_slice(&handshake);

//     // Update hello length
//     let hello_length = (hello.len() - 5) as u16;
//     hello[3..5].copy_from_slice(&hello_length.to_be_bytes());

//     // // Send the fake Client Hello
//     // stream.write_all(&hello).await?;

//     // Отправляем только первые 100 байт фейкового Client Hello
//     let truncated_hello = &hello[..std::cmp::min(hello.len(), 100)];
//     stream.write_all(truncated_hello).await?;

//     // Reset TTL to default value (64 is a common default)
//     let default_ttl = 64 as c_int;
//     unsafe {
//         if setsockopt(
//             fd,
//             IPPROTO_IP,
//             IP_TTL,
//             &default_ttl as *const _ as *const _,
//             std::mem::size_of_val(&default_ttl) as u32,
//         ) != 0
//         {
//             return Err("Failed to reset TTL".into());
//         }
//     }

//     Ok(())
// }

const FAKE_CLIENTHELLO_PART0: &[u8] = &[
    0x16, 0x03, 0x01, 0xDD, 0xDD, 0x01, 0x00, 0xDD, 0xDD, 0x03, 0x03, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0x20, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0x00, 0x22, 0x13, 0x01,
    0x13, 0x03, 0x13, 0x02, 0xC0, 0x2B, 0xC0, 0x2F, 0xCC, 0xA9, 0xCC, 0xA8, 0xC0, 0x2C, 0xC0, 0x30,
    0xC0, 0x0A, 0xC0, 0x09, 0xC0, 0x13, 0xC0, 0x14, 0x00, 0x9C, 0x00, 0x9D, 0x00, 0x2F, 0x00, 0x35,
    0x01, 0x00, 0xDD, 0xDD,
];

const FAKE_CLIENTHELLO_PART1: &[u8] = &[
    // extended_master_secret
    0x00, 0x17, 0x00, 0x00, // renegotiation_info
    0xFF, 0x01, 0x00, 0x01, 0x00, // supported_groups
    0x00, 0x0A, 0x00, 0x0E, 0x00, 0x0C, 0x00, 0x1D, 0x00, 0x17, 0x00, 0x18, 0x00, 0x19, 0x01, 0x00,
    0x01, 0x01, // ex_point_formats
    0x00, 0x0B, 0x00, 0x02, 0x01, 0x00, // session_ticket
    0x00, 0x23, 0x00, 0x00, // ALPN
    0x00, 0x10, 0x00, 0x0E, 0x00, 0x0C, 0x02, 0x68, 0x32, 0x08, 0x68, 0x74, 0x74, 0x70, 0x2F, 0x31,
    0x2E, 0x31, // status_request
    0x00, 0x05, 0x00, 0x05, 0x01, 0x00, 0x00, 0x00, 0x00, // delegated_credentials
    0x00, 0x22, 0x00, 0x0A, 0x00, 0x08, 0x04, 0x03, 0x05, 0x03, 0x06, 0x03, 0x02, 0x03,
    // key_share
    0x00, 0x33, 0x00, 0x6B, 0x00, 0x69, 0x00, 0x1D, 0x00, 0x20, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0x00, 0x17, 0x00, 0x41, 0x04, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    // supported_versions
    0x00, 0x2B, 0x00, 0x05, 0x04, 0x03, 0x04, 0x03, 0x03, // signature_algorithms
    0x00, 0x0D, 0x00, 0x18, 0x00, 0x16, 0x04, 0x03, 0x05, 0x03, 0x06, 0x03, 0x08, 0x04, 0x08, 0x05,
    0x08, 0x06, 0x04, 0x01, 0x05, 0x01, 0x06, 0x01, 0x02, 0x03, 0x02, 0x01,
    // psk_key_exchange_modes
    0x00, 0x2D, 0x00, 0x02, 0x01, 0x01, // record_size_limit
    0x00, 0x1C, 0x00, 0x02, 0x40, 0x01, // encrypted_client_hello
    0xFE, 0x0D, 0x01, 0x19, 0x00, 0x00, 0x01, 0x00, 0x01, 0xAA, 0x00, 0x20, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0x00, 0xEF, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
];

async fn send_fake_client_hello<T: AsyncWrite + AsRawFd + Unpin>(
    stream: &mut T,
    sni: &str,
    ttl: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    // Set TTL for the fake packet
    let fd = stream.as_raw_fd();
    let ttl = ttl as c_int;
    unsafe {
        if setsockopt(
            fd,
            IPPROTO_IP,
            IP_TTL,
            &ttl as *const _ as *const _,
            std::mem::size_of_val(&ttl) as u32,
        ) != 0
        {
            return Err("Failed to set TTL".into());
        }
    }

    let mut rng = OsRng;

    // Calculate sizes
    let name_size = sni.len();
    let part0_size = FAKE_CLIENTHELLO_PART0.len();
    let part1_size = FAKE_CLIENTHELLO_PART1.len();
    let sni_head_size = 9;
    let packet_size = part0_size + part1_size + sni_head_size + name_size;

    // Create packet
    let mut packet = vec![0u8; packet_size];

    // Copy major parts of packet
    packet[..part0_size].copy_from_slice(FAKE_CLIENTHELLO_PART0);
    packet[part0_size + sni_head_size + name_size..].copy_from_slice(FAKE_CLIENTHELLO_PART1);

    // Replace placeholders with random generated values
    for byte in packet.iter_mut() {
        if *byte == 0xAA {
            *byte = rng.gen::<u8>();
        }
    }

    // Write size fields into packet
    set_uint16be(&mut packet, 3, (packet_size - 5) as u16);
    set_uint16be(&mut packet, 7, (packet_size - 9) as u16);
    set_uint16be(&mut packet, 0x72, (packet_size - 116) as u16);

    // Write SNI extension
    set_uint16be(&mut packet, part0_size, 0);
    set_uint16be(&mut packet, part0_size + 2, (name_size + 5) as u16);
    set_uint16be(&mut packet, part0_size + 4, (name_size + 3) as u16);
    packet[part0_size + 6] = 0;
    set_uint16be(&mut packet, part0_size + 7, name_size as u16);
    packet[part0_size + sni_head_size..part0_size + sni_head_size + name_size]
        .copy_from_slice(sni.as_bytes());

    // Send the fake Client Hello
    // stream.write_all(&packet).await?;

    // Отправляем только первые 100 байт фейкового Client Hello
    // let truncated_packet = &packet[..std::cmp::min(packet.len(), 100)];
    // stream.write_all(truncated_packet).await?;

    match stream.write_all(&packet).await {
        Ok(_) => println!("Fake Client Hello sent successfully"),
        Err(e) => println!("Error sending fake Client Hello: {}", e),
    }

    // Reset TTL to default value (64 is a common default)
    let default_ttl = 64 as c_int;
    unsafe {
        if setsockopt(
            fd,
            IPPROTO_IP,
            IP_TTL,
            &default_ttl as *const _ as *const _,
            std::mem::size_of_val(&default_ttl) as u32,
        ) != 0
        {
            return Err("Failed to reset TTL".into());
        }
    }

    Ok(())
}

fn set_uint16be(buffer: &mut [u8], offset: usize, value: u16) {
    buffer[offset] = (value >> 8) as u8;
    buffer[offset + 1] = value as u8;
}
