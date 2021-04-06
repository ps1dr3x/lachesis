use std::{
    net::SocketAddr,
    sync::mpsc::Sender,
    time::{Duration, Instant},
};

use bytes::Buf;
use hyper::{
    client::{Client, HttpConnector},
    Body, Request, Uri,
};
use hyper_tls::HttpsConnector;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};
use tokio_native_tls::TlsConnector;

use super::worker::{PortStatus, PortTarget, ReqTarget, WorkerMessage};

pub async fn test_port(ip: String, port: u16, timeout_millis: u64) -> PortTarget {
    let addr = format!("{}:{}", ip, port).parse::<SocketAddr>().unwrap();
    let mut port_target = PortTarget {
        ip,
        port,
        status: PortStatus::Closed,
        time: Instant::now(),
    };

    match timeout(
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

pub struct HttpsRequest {
    pub tx: Sender<WorkerMessage>,
    pub client: Client<HttpsConnector<HttpConnector>>,
    pub target: ReqTarget,
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
                    .send(WorkerMessage::Fail(
                        req.target.clone(),
                        "Request error".to_string(),
                        Some(e.to_string()),
                    ))
                    .unwrap();
                return;
            }
        };

        match hyper::body::aggregate(body).await {
            Ok(mut b) => {
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
                let mut bytes = Vec::new();
                b.copy_to_slice(&mut bytes);
                raw_content = format!("{}\r\n{}", raw_content, String::from_utf8_lossy(&bytes));
                req.target.response = raw_content;

                req.tx
                    .send(WorkerMessage::Response(req.target.clone()))
                    .unwrap();
            }
            Err(e) => {
                req.tx
                    .send(WorkerMessage::Fail(
                        req.target.clone(),
                        "Response error".to_string(),
                        Some(e.to_string()),
                    ))
                    .unwrap();
            }
        };
    };
    if timeout(to, cb).await.is_err() {
        req.tx
            .send(WorkerMessage::Timeout(req.target.clone()))
            .unwrap();
    }
}

pub struct TcpRequest {
    pub tx: Sender<WorkerMessage>,
    pub target: ReqTarget,
    pub message: String,
    pub timeout: u64,
}

pub async fn tcp_custom(req: TcpRequest) {
    let mut req = req;

    let addr = match format!("{}:{}", req.target.ip, req.target.port).parse::<SocketAddr>() {
        Ok(addr) => addr,
        Err(_e) => {
            req.tx
                .send(WorkerMessage::Fail(
                    req.target,
                    "Invalid address".to_string(),
                    None,
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
                    .send(WorkerMessage::Fail(
                        req.target.clone(),
                        "TCP stream connection error".to_string(),
                        Some(e.to_string()),
                    ))
                    .unwrap();
                return;
            }
        };

        if let Err(e) = stream.write_all(req.message.as_bytes()).await {
            req.tx
                .send(WorkerMessage::Fail(
                    req.target.clone(),
                    "TCP stream write error".to_string(),
                    Some(e.to_string()),
                ))
                .unwrap();
            return;
        }

        // FIXME - improve the way how the answer is read
        let mut answer = [0; 100_000];
        if let Err(e) = stream.read(&mut answer).await {
            req.tx
                .send(WorkerMessage::Fail(
                    req.target.clone(),
                    "TCP stream read error".to_string(),
                    Some(e.to_string()),
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
            .send(WorkerMessage::Timeout(req.target.clone()))
            .unwrap();
    };
}
