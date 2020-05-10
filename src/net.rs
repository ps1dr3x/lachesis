use std::{
    net::SocketAddr,
    sync::mpsc::Sender,
    time::{Duration, Instant},
};

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

pub struct HttpsRequest {
    pub tx: Sender<WorkerMessage>,
    pub client: Client<HttpsConnector<HttpConnector>>,
    pub target: Target,
    pub protocol: String,
    pub port: u16,
    pub user_agent: String,
    pub timeout: u64,
}

pub async fn http_s(req: HttpsRequest) {
    let mut target = req.target.clone();
    target.protocol = req.protocol;
    target.port = req.port;
    target.time = Instant::now();

    let uri: Uri = format!("{}://{}:{}", target.protocol, target.ip, target.port)
        .parse()
        .unwrap();

    let request = Request::builder()
        .uri(uri)
        .header("Host", target.domain.clone())
        .header("User-Agent", req.user_agent.clone())
        .header("Accept", "*/*")
        .body(Body::empty())
        .unwrap();

    let request = match timeout(
        Duration::from_secs(req.timeout / 2),
        req.client.request(request),
    )
    .await
    {
        Ok(r) => r,
        Err(_) => {
            req.tx
                .send(WorkerMessage::Timeout(
                    format!(
                        "[{}][{}:{}] - Request timeout",
                        target.protocol.to_uppercase().blue(),
                        target.domain.cyan(),
                        target.port.to_string().cyan(),
                    ),
                    target.protocol,
                ))
                .unwrap();
            return;
        }
    };

    let (parts, body) = match request {
        Ok(r) => r.into_parts(),
        Err(e) => {
            req.tx
                .send(WorkerMessage::Error(
                    format!(
                        "[{}][{}:{}] - Request error: {}",
                        target.protocol.to_uppercase().blue(),
                        target.domain.cyan(),
                        target.port.to_string().cyan(),
                        e
                    ),
                    target.protocol,
                ))
                .unwrap();
            return;
        }
    };

    let body = match timeout(
        Duration::from_secs(req.timeout / 2),
        hyper::body::aggregate(body),
    )
    .await
    {
        Ok(a) => a,
        Err(_) => {
            req.tx
                .send(WorkerMessage::Timeout(
                    format!(
                        "[{}][{}:{}] - Response body timeout",
                        target.protocol.to_uppercase().blue(),
                        target.domain.cyan(),
                        target.port.to_string().cyan(),
                    ),
                    target.protocol,
                ))
                .unwrap();
            return;
        }
    };

    match body {
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
            target.response = raw_content;

            req.tx.send(WorkerMessage::Response(target)).unwrap();
        }
        Err(e) => {
            req.tx
                .send(WorkerMessage::Error(
                    format!(
                        "[{}][{}:{}] - Response error: {}",
                        target.protocol.to_uppercase().blue(),
                        target.domain.cyan(),
                        target.port.to_string().cyan(),
                        e
                    ),
                    target.protocol,
                ))
                .unwrap();
        }
    };
}

pub struct TcpRequest {
    pub tx: Sender<WorkerMessage>,
    pub target: Target,
    pub port: u16,
    pub message: String,
    pub timeout: u64,
}

pub async fn tcp_custom(req: TcpRequest) {
    let mut target = req.target.clone();
    target.domain = String::new();
    target.protocol = "tcp/custom".to_string();
    target.port = req.port;
    target.time = Instant::now();

    let addr = match format!("{}:{}", target.ip, target.port).parse::<SocketAddr>() {
        Ok(addr) => addr,
        Err(_e) => {
            req.tx
                .send(WorkerMessage::Error(
                    format!(
                        "[{}] Invalid address: {}:{}",
                        target.protocol.to_uppercase().blue(),
                        target.ip.cyan(),
                        req.port.to_string().cyan()
                    ),
                    target.protocol,
                ))
                .unwrap();
            return;
        }
    };

    let stream = match timeout(
        Duration::from_secs(req.timeout / 3),
        TcpStream::connect(&addr),
    )
    .await
    {
        Ok(s) => s,
        Err(_) => {
            req.tx
                .send(WorkerMessage::Timeout(
                    format!(
                        "[{}][{}:{}] - Tcp connection timeout",
                        target.protocol.to_uppercase().blue(),
                        target.ip.cyan(),
                        target.port.to_string().cyan(),
                    ),
                    target.protocol,
                ))
                .unwrap();
            return;
        }
    };

    let mut stream = match stream {
        Ok(s) => s,
        Err(e) => {
            req.tx
                .send(WorkerMessage::Error(
                    format!(
                        "[{}][{}:{}] - TCP stream connection error: {}",
                        target.protocol.to_uppercase().blue(),
                        target.ip.cyan(),
                        target.port.to_string().cyan(),
                        e
                    ),
                    target.protocol,
                ))
                .unwrap();
            return;
        }
    };

    match timeout(
        Duration::from_secs(req.timeout / 3),
        stream.write_all(req.message.as_bytes()),
    )
    .await
    {
        Ok(w) => {
            if let Err(e) = w {
                req.tx
                    .send(WorkerMessage::Error(
                        format!(
                            "[{}][{}:{}] - TCP stream write error: {}",
                            target.protocol.to_uppercase().blue(),
                            target.ip.cyan(),
                            target.port.to_string().cyan(),
                            e
                        ),
                        target.protocol,
                    ))
                    .unwrap();
                return;
            }
        }
        Err(_) => {
            req.tx
                .send(WorkerMessage::Timeout(
                    format!(
                        "[{}][{}:{}] - Tcp stream write timeout",
                        target.protocol.to_uppercase().blue(),
                        target.ip.cyan(),
                        target.port.to_string().cyan(),
                    ),
                    target.protocol,
                ))
                .unwrap();
            return;
        }
    };

    // FIXME - find a better way to read the answer
    let mut answer = [0; 100_000];
    match timeout(
        Duration::from_secs(req.timeout / 3),
        stream.read(&mut answer),
    )
    .await
    {
        Ok(w) => {
            if let Err(e) = w {
                req.tx
                    .send(WorkerMessage::Error(
                        format!(
                            "[{}][{}:{}] - TCP stream read error: {}",
                            target.protocol.to_uppercase().blue(),
                            target.ip.cyan(),
                            target.port.to_string().cyan(),
                            e
                        ),
                        target.protocol,
                    ))
                    .unwrap();
                return;
            }
        }
        Err(_) => {
            req.tx
                .send(WorkerMessage::Timeout(
                    format!(
                        "[{}][{}:{}] - Tcp stream read timeout",
                        target.protocol.to_uppercase().blue(),
                        target.ip.cyan(),
                        target.port.to_string().cyan(),
                    ),
                    target.protocol,
                ))
                .unwrap();
            return;
        }
    };

    target.response = String::from_utf8_lossy(&answer).to_string();
    req.tx.send(WorkerMessage::Response(target)).unwrap();
}
