use std::sync::Arc;

use super::ProxyConfig;
use crate::server::tunnel::{tunnel_direct, tunnel_via_proxy};
use crate::server::utils::tracing_error;

use bytes::Bytes;
use http::{Method, Request, Response};
use http_body_util::{combinators::BoxBody, Empty};
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1::Builder;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use wildmatch::WildMatch;

pub async fn handle_request(
    req: Request<hyper::body::Incoming>,
    proxy_config: Arc<Vec<ProxyConfig>>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    // HTTPS request
    if Method::CONNECT == req.method() {
        if let Some(addr) = host_addr(req.uri()) {
            let host = req.uri().host().unwrap().to_string();
            let found_proxy = find_matching_proxy(Arc::clone(&proxy_config).as_ref(), &host);

            tokio::spawn(async move {
                match hyper::upgrade::on(req).await {
                    Ok(upgraded) => {
                        if let Some((proxy, filter_name)) = found_proxy {
                            if let Err(e) =
                                tunnel_via_proxy(upgraded, &addr, proxy, &filter_name).await
                            {
                                tracing_error(&format!("{addr} → PROXY connection error: {e}"));
                            };
                        } else if let Err(e) = tunnel_direct(upgraded, &addr).await {
                            tracing_error(&format!("{addr} → DIRECT connection error: {e}"));
                        }
                    }
                    Err(e) => tracing_error(&format!("{addr} → UPGRADE error: {e}")),
                }
            });

            Ok(Response::new(empty()))
        } else {
            tracing_error(&format!("CONNECT host is not socket addr: {:?}", req.uri()));
            let mut resp = Response::new(full("CONNECT must be to a socket address"));
            *resp.status_mut() = http::StatusCode::BAD_REQUEST;

            Ok(resp)
        }
    } else {
        // HTTP request
        // TODO: Implement HTTP request proxying through HTTP proxy

        tracing::info!("{:?} {:?} → DIRECT connection", req.method(), req.uri());

        let host = req.uri().host().expect("uri has no host");
        let port = req.uri().port_u16().unwrap_or(80);

        match TcpStream::connect((host, port)).await {
            Ok(stream) => {
                let (mut sender, conn) = Builder::new()
                    .preserve_header_case(true)
                    .title_case_headers(true)
                    .handshake(TokioIo::new(stream))
                    .await?;

                tokio::spawn(async move {
                    if let Err(err) = conn.await {
                        tracing_error(&format!("Connection failed: {:?}", err));
                    }
                });

                let resp = sender.send_request(req).await?;
                Ok(resp.map(|b| b.boxed()))
            }
            Err(err) => {
                tracing_error(&format!("{host}:{port} → Failed to connect: {:?}", err));

                let mut resp = Response::new(full("Failed to connect to host"));
                *resp.status_mut() = http::StatusCode::BAD_GATEWAY;

                Ok(resp)
            }
        }
    }
}

fn empty() -> BoxBody<Bytes, hyper::Error> {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}

fn full<T: Into<Bytes>>(chunk: T) -> BoxBody<Bytes, hyper::Error> {
    Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}

fn host_addr(uri: &http::Uri) -> Option<String> {
    uri.authority().map(|auth| auth.to_string())
}

fn is_host_allowed(req_host: &str, allowed_hosts: &[String]) -> bool {
    for allowed_host in allowed_hosts {
        if WildMatch::new(allowed_host).matches(req_host) {
            return true;
        }
    }
    false
}

fn find_matching_proxy(
    proxy_config: &[ProxyConfig],
    req_host: &str,
) -> Option<(ProxyConfig, String)> {
    for proxy in proxy_config.iter() {
        if !proxy.enabled {
            continue;
        }

        for filter in proxy.filter.iter() {
            if is_host_allowed(req_host, &filter.domains) {
                return Some((proxy.clone(), filter.name.clone()));
            }
        }
    }

    None
}
