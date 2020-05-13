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

pub async fn test_port(ip: String, port: u16) -> bool {
    let addr = match format!("{}:{}", ip, port).parse::<SocketAddr>() {
        Ok(addr) => addr,
        Err(_) => return false,
    };

    match timeout(Duration::from_secs(3), TcpStream::connect(&addr)).await {
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
                        req.target.protocol.to_uppercase().blue(),
                        req.target.domain.cyan(),
                        req.target.port.to_string().cyan(),
                    ),
                    req.target.protocol,
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
                        req.target.protocol.to_uppercase().blue(),
                        req.target.domain.cyan(),
                        req.target.port.to_string().cyan(),
                        e
                    ),
                    req.target.protocol,
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
                        req.target.protocol.to_uppercase().blue(),
                        req.target.domain.cyan(),
                        req.target.port.to_string().cyan(),
                    ),
                    req.target.protocol,
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
            req.target.response = raw_content;

            req.tx.send(WorkerMessage::Response(req.target)).unwrap();
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
                    req.target.protocol,
                ))
                .unwrap();
        }
    };
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
                    req.target.protocol,
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
                        req.target.protocol.to_uppercase().blue(),
                        req.target.ip.cyan(),
                        req.target.port.to_string().cyan(),
                    ),
                    req.target.protocol,
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
                        req.target.protocol.to_uppercase().blue(),
                        req.target.ip.cyan(),
                        req.target.port.to_string().cyan(),
                        e
                    ),
                    req.target.protocol,
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
                            req.target.protocol.to_uppercase().blue(),
                            req.target.ip.cyan(),
                            req.target.port.to_string().cyan(),
                            e
                        ),
                        req.target.protocol,
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
                        req.target.protocol.to_uppercase().blue(),
                        req.target.ip.cyan(),
                        req.target.port.to_string().cyan(),
                    ),
                    req.target.protocol,
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
                            req.target.protocol.to_uppercase().blue(),
                            req.target.ip.cyan(),
                            req.target.port.to_string().cyan(),
                            e
                        ),
                        req.target.protocol,
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
                        req.target.protocol.to_uppercase().blue(),
                        req.target.ip.cyan(),
                        req.target.port.to_string().cyan(),
                    ),
                    req.target.protocol,
                ))
                .unwrap();
            return;
        }
    };

    req.target.response = String::from_utf8_lossy(&answer).to_string();
    req.tx.send(WorkerMessage::Response(req.target)).unwrap();
}
