use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};

use reqwest::{redirect, Method};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc::Sender,
    time,
};

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

pub fn build_https_client() -> reqwest::Client {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .redirect(redirect::Policy::none())
        .build()
        .expect("failed to build HTTP client")
}

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
pub struct HttpsOptions {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub payload: String,
}

pub async fn http_s(
    tx: Sender<WorkerMessage>,
    client: reqwest::Client,
    mut target: ReqTarget,
    options: HttpsOptions,
    user_agent: String,
    timeout: u64,
    max_response_size: usize,
) {
    let url = format!(
        "{}://{}:{}{}",
        target.protocol, target.ip, target.port, options.path
    );

    let method = Method::from_bytes(options.method.as_bytes()).unwrap_or(Method::GET);
    let time_limit = Duration::from_secs(timeout);

    let request = async {
        let mut req = client
            .request(method, &url)
            .header("User-Agent", &user_agent)
            .header("Accept", "*/*");

        if !target.domain.is_empty() {
            req = req.header("Host", &target.domain);
        }

        for (k, v) in &options.headers {
            req = req.header(k, v);
        }

        if !options.payload.is_empty() {
            req = req.body(options.payload.clone());
        }

        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                let version = resp.version();
                let headers = resp.headers().clone();

                match resp.bytes().await {
                    Ok(b) => {
                        let mut raw_content = format!("{:?} {}\r\n", version, status);
                        for (name, value) in &headers {
                            raw_content = format!(
                                "{}{}: {}\r\n",
                                raw_content,
                                name,
                                value.to_str().unwrap_or("")
                            );
                        }
                        let body = &b[..b.len().min(max_response_size)];
                        raw_content =
                            format!("{}\r\n{}", raw_content, String::from_utf8_lossy(body));

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
                }
            }
            Err(e) => {
                tx.send(WorkerMessage::Fail(
                    target.clone(),
                    "Request error".to_string(),
                    Some(e.to_string()),
                ))
                .await
                .unwrap();
            }
        }
    };

    if time::timeout(time_limit, request).await.is_err() {
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
    max_response_size: usize,
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
        let mut response = vec![0; max_response_size];
        let mut response_length = 0;
        loop {
            stream.readable().await.unwrap();

            match stream.read(&mut response).await {
                Ok(0) => break,
                Ok(n) => {
                    response_length += n;
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

        if response_length > 0 {
            response.truncate(response_length);
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
