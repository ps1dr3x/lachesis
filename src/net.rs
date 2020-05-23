use std::{net::SocketAddr, sync::mpsc::Sender, time::Duration};

use bytes::Buf;
use colored::Colorize;
use hyper::{client::HttpConnector, Body, Client, Request, Uri};
use hyper_tls::HttpsConnector;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

use super::worker::{Target, WorkerMessage};

pub async fn test_port(ip: String, port: u16, timeout_millis: u64) -> bool {
    let addr = match format!("{}:{}", ip, port).parse::<SocketAddr>() {
        Ok(addr) => addr,
        Err(_) => return false,
    };

    match timeout(
        Duration::from_millis(timeout_millis),
        TcpStream::connect(&addr),
    )
    .await
    {
        Ok(s) => s.is_ok(),
        Err(_) => false,
    }
}

pub struct HttpsRequest {
    pub tx: Sender<WorkerMessage>,
    pub client: Client<HttpsConnector<HttpConnector>>,
    pub target: Target,
    pub user_agent: String,
    pub timeout: u64,
}

pub async fn http_s(req: HttpsRequest) {
    let mut req = req;

    let uri: Uri = format!(
        "{}://{}:{}",
        req.target.protocol, req.target.ip, req.target.port
    )
    .parse()
    .unwrap();

    let request = Request::builder()
        .uri(uri)
        .header("Host", req.target.domain.clone())
        .header("User-Agent", req.user_agent.clone())
        .header("Accept", "*/*")
        .body(Body::empty())
        .unwrap();

    let to = Duration::from_secs(req.timeout);
    let cb = async {
        let (parts, body) = match req.client.request(request).await {
            Ok(r) => r.into_parts(),
            Err(e) => {
                req.tx
                    .send(WorkerMessage::Error(
                        format!(
                            "[{}][{}:{}] - Request error: {}",
                            req.target.protocol.to_uppercase().blue(),
                            req.target.domain.cyan(),
                            req.target.port.to_string().cyan(),
                            e
                        ),
                        req.target.protocol.clone(),
                    ))
                    .unwrap();
                return;
            }
        };

        match hyper::body::aggregate(body).await {
            Ok(b) => {
                // Merge response's headers and body
                let mut raw_content = format!("{:?} {}\r\n", parts.version, parts.status);
                for header in &parts.headers {
                    raw_content = format!(
                        "{}{}: {}\r\n",
                        raw_content,
                        header.0,
                        header.1.to_str().unwrap_or("")
                    );
                }
                raw_content = format!("{}\r\n{}", raw_content, String::from_utf8_lossy(b.bytes()));
                req.target.response = raw_content;

                req.tx
                    .send(WorkerMessage::Response(req.target.clone()))
                    .unwrap();
            }
            Err(e) => {
                req.tx
                    .send(WorkerMessage::Error(
                        format!(
                            "[{}][{}:{}] - Response error: {}",
                            req.target.protocol.to_uppercase().blue(),
                            req.target.domain.cyan(),
                            req.target.port.to_string().cyan(),
                            e
                        ),
                        req.target.protocol.clone(),
                    ))
                    .unwrap();
            }
        };
    };
    if timeout(to, cb).await.is_err() {
        req.tx
            .send(WorkerMessage::Timeout(
                format!(
                    "[{}][{}:{}] - Request timeout",
                    req.target.protocol.to_uppercase().blue(),
                    req.target.domain.cyan(),
                    req.target.port.to_string().cyan(),
                ),
                req.target.protocol,
            ))
            .unwrap();
    }
}

pub struct TcpRequest {
    pub tx: Sender<WorkerMessage>,
    pub target: Target,
    pub message: String,
    pub timeout: u64,
}

pub async fn tcp_custom(req: TcpRequest) {
    let mut req = req;

    let addr = match format!("{}:{}", req.target.ip, req.target.port).parse::<SocketAddr>() {
        Ok(addr) => addr,
        Err(_e) => {
            req.tx
                .send(WorkerMessage::Error(
                    format!(
                        "[{}] Invalid address: {}:{}",
                        req.target.protocol.to_uppercase().blue(),
                        req.target.ip.cyan(),
                        req.target.port.to_string().cyan()
                    ),
                    req.target.protocol.clone(),
                ))
                .unwrap();
            return;
        }
    };

    let to = Duration::from_secs(req.timeout);
    let cb = async {
        let mut stream = match TcpStream::connect(&addr).await {
            Ok(s) => s,
            Err(e) => {
                req.tx
                    .send(WorkerMessage::Error(
                        format!(
                            "[{}][{}:{}] - TCP stream connection error: {}",
                            req.target.protocol.to_uppercase().blue(),
                            req.target.ip.cyan(),
                            req.target.port.to_string().cyan(),
                            e
                        ),
                        req.target.protocol.clone(),
                    ))
                    .unwrap();
                return;
            }
        };

        if let Err(e) = stream.write_all(req.message.as_bytes()).await {
            req.tx
                .send(WorkerMessage::Error(
                    format!(
                        "[{}][{}:{}] - TCP stream write error: {}",
                        req.target.protocol.to_uppercase().blue(),
                        req.target.ip.cyan(),
                        req.target.port.to_string().cyan(),
                        e
                    ),
                    req.target.protocol.clone(),
                ))
                .unwrap();
            return;
        }

        // FIXME - improve the way how the answer is read
        let mut answer = [0; 100_000];
        if let Err(e) = stream.read(&mut answer).await {
            req.tx
                .send(WorkerMessage::Error(
                    format!(
                        "[{}][{}:{}] - TCP stream read error: {}",
                        req.target.protocol.to_uppercase().blue(),
                        req.target.ip.cyan(),
                        req.target.port.to_string().cyan(),
                        e
                    ),
                    req.target.protocol.clone(),
                ))
                .unwrap();
            return;
        }
        req.target.response = String::from_utf8_lossy(&answer).to_string();
        req.tx
            .send(WorkerMessage::Response(req.target.clone()))
            .unwrap();
    };

    if timeout(to, cb).await.is_err() {
        req.tx
            .send(WorkerMessage::Timeout(
                format!(
                    "[{}][{}:{}] - Tcp connection timeout",
                    req.target.protocol.to_uppercase().blue(),
                    req.target.ip.cyan(),
                    req.target.port.to_string().cyan(),
                ),
                req.target.protocol,
            ))
            .unwrap();
    };
}