use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};

use hyper::{
    client::{Client, HttpConnector},
    Body, Method, Request, Uri,
};
use hyper_tls::HttpsConnector;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc::Sender,
    time,
};
use tokio_native_tls::TlsConnector;

use super::worker::{PortStatus, PortTarget, ReqTarget, WorkerMessage};

pub async fn test_port(ip: String, port: u16, timeout_millis: u64) -> PortTarget {
    let addr = format!("{}:{}", ip, port).parse::<SocketAddr>().unwrap();
    let mut port_target = PortTarget {
        port,
        status: PortStatus::Closed,
        time: Instant::now(),
    };

    match time::timeout(
        Duration::from_millis(timeout_millis),
        TcpStream::connect(&addr),
    )
    .await
    {
        Ok(s) => match s {
            Ok(_) => {
                port_target.status = PortStatus::Open;
                port_target
            }
            Err(_) => port_target,
        },
        Err(_) => {
            port_target.status = PortStatus::Timedout;
            port_target
        }
    }
}

pub fn build_https_client() -> Client<HttpsConnector<HttpConnector>> {
    // TODOs:
    // - Tweak connectors and client configuration
    // - Try using rustls instead of native_tls as TLS connector
    let mut http = HttpConnector::new();
    //http.set_connect_timeout(Some(Duration::from_millis(1000)));
    http.enforce_http(false);
    let tls_connector = native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
    let tls_connector = TlsConnector::from(tls_connector);
    let https = HttpsConnector::from((http, tls_connector));
    Client::builder()
        //.pool_idle_timeout(Duration::from_millis(1250))
        //.http2_keep_alive_timeout(Duration::from_millis(1000))
        //.retry_canceled_requests(false)
        .build(https)
}

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
pub struct HttpsOptions {
    pub method: String,
    pub path: String,
    pub payload: String,
}

pub async fn http_s(
    tx: Sender<WorkerMessage>,
    client: Client<HttpsConnector<HttpConnector>>,
    mut target: ReqTarget,
    options: HttpsOptions,
    user_agent: String,
    timeout: u64,
) {
    let uri: Uri = format!(
        "{}://{}:{}{}",
        target.protocol, target.ip, target.port, options.path
    )
    .parse()
    .unwrap();

    let request = Request::builder()
        .uri(uri)
        .method(Method::from_bytes(options.method.as_bytes()).unwrap())
        .header("Host", target.domain.clone())
        .header("User-Agent", user_agent.clone())
        .header("Accept", "*/*")
        .body(Body::from(options.payload))
        .unwrap();

    let time = Duration::from_secs(timeout);
    let request = async {
        let (parts, body) = match client.request(request).await {
            Ok(r) => r.into_parts(),
            Err(e) => {
                tx.send(WorkerMessage::Fail(
                    target.clone(),
                    "Request error".to_string(),
                    Some(e.to_string()),
                ))
                .await
                .unwrap();
                return;
            }
        };

        match hyper::body::to_bytes(body).await {
            Ok(b) => {
                // Merge response's headers and body (UTF-8)
                let mut raw_content = format!("{:?} {}\r\n", parts.version, parts.status);
                for (name, value) in &parts.headers {
                    raw_content = format!(
                        "{}{}: {}\r\n",
                        raw_content,
                        name,
                        value.to_str().unwrap_or("")
                    );
                }
                raw_content = format!("{}\r\n{}", raw_content, String::from_utf8_lossy(&b));

                target.response = raw_content;

                tx.send(WorkerMessage::Response(target.clone()))
                    .await
                    .unwrap();
            }
            Err(e) => {
                tx.send(WorkerMessage::Fail(
                    target.clone(),
                    "Response error".to_string(),
                    Some(e.to_string()),
                ))
                .await
                .unwrap();
            }
        };
    };

    if time::timeout(time, request).await.is_err() {
        tx.send(WorkerMessage::Timeout(target.clone()))
            .await
            .unwrap();
    }
}

pub async fn tcp_custom(
    tx: Sender<WorkerMessage>,
    mut target: ReqTarget,
    payload: String,
    timeout: u64,
) {
    let addr = match format!("{}:{}", target.ip, target.port).parse::<SocketAddr>() {
        Ok(addr) => addr,
        Err(_e) => {
            tx.send(WorkerMessage::Fail(
                target,
                "Invalid address".to_string(),
                None,
            ))
            .await
            .unwrap();
            return;
        }
    };

    let to = Duration::from_secs(timeout);
    let cb = async {
        let mut stream = match TcpStream::connect(&addr).await {
            Ok(s) => s,
            Err(e) => {
                tx.send(WorkerMessage::Fail(
                    target.clone(),
                    "TCP stream connection error".to_string(),
                    Some(e.to_string()),
                ))
                .await
                .unwrap();
                return;
            }
        };

        stream.writable().await.unwrap();
        if let Err(e) = stream.write_all(payload.as_bytes()).await {
            tx.send(WorkerMessage::Fail(
                target.clone(),
                "TCP stream write error".to_string(),
                Some(e.to_string()),
            ))
            .await
            .unwrap();
            return;
        }

        // TODO - configurable max response size
        let mut response = vec![0; 10240];
        let mut response_lenght = 0;
        loop {
            stream.readable().await.unwrap();

            match stream.read(&mut response).await {
                Ok(n) if n == 0 => break,
                Ok(n) => {
                    response_lenght += n;
                }
                Err(e) => {
                    tx.send(WorkerMessage::Fail(
                        target.clone(),
                        "TCP stream read error".to_string(),
                        Some(e.to_string()),
                    ))
                    .await
                    .unwrap();
                    return;
                }
            };
        }

        if response_lenght > 0 {
            response.truncate(response_lenght);
            target.response = String::from_utf8_lossy(&response).to_string();
            tx.send(WorkerMessage::Response(target.clone()))
                .await
                .unwrap();
        }
    };

    if time::timeout(to, cb).await.is_err() {
        tx.send(WorkerMessage::Timeout(target.clone()))
            .await
            .unwrap();
    };
}
