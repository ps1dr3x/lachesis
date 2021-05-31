use std::{convert::Infallible, fs, net::SocketAddr};

use hyper::{
    service::{make_service_fn, service_fn},
    Body, Request, Response, Server,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    runtime,
};

use crate::{
    conf::{self, Conf, DbConf},
    db::DbMan,
    lachesis,
};

async fn test_server_tcp() {
    let listener = TcpListener::bind("0.0.0.0:4000").await.unwrap();

    loop {
        let (mut socket, _) = listener.accept().await.unwrap();

        tokio::spawn(async move {
            let mut msg = vec![0; 1024];
            loop {
                socket.readable().await.unwrap();

                match socket.read(&mut msg).await {
                    Ok(n) if n == 0 => continue,
                    Ok(n) => {
                        msg.truncate(n);
                        // println!("Payload: {}", String::from_utf8_lossy(&msg).to_string());
                        break;
                    }
                    Err(e) => panic!("TCP server error {}", e),
                };
            }

            let content = fs::read_to_string("./resources/test.html").unwrap();
            socket.writable().await.unwrap();
            socket.write_all(content.as_bytes()).await.unwrap();
        });
    }
}

async fn test_html(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let contents = fs::read_to_string("./resources/test.html").unwrap();
    Ok(Response::new(contents.into()))
}

async fn test_server_http() {
    let addr = SocketAddr::from(([0, 0, 0, 0], 4001));

    let make_svc = make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(test_html)) });

    let server = Server::bind(&addr).serve(make_svc);

    if let Err(e) = server.await {
        panic!("HTTP server error: {}", e);
    }
}

fn test_conf() -> Conf {
    let mut conf = Conf::default();
    conf.db_conf = DbConf {
        host: "127.0.0.1".to_string(),
        port: "5432".to_string(),
        dbname: "lachesis_dev".to_string(),
        user: "lachesis_agent".to_string(),
        password: "insecure".to_string(),
    };
    conf.dataset = "./resources/test-dataset.json".to_string();
    conf.definitions = conf::parse_validate_definitions(&[
        "./resources/test-definition-http.json".to_string(),
        "./resources/test-definition-tcp.json".to_string(),
    ])
    .unwrap();
    conf
}

#[tokio::test]
async fn test_overall() {
    let rt = runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.spawn(test_server_http());
    rt.spawn(test_server_tcp());

    let mut conf = test_conf();
    conf.max_targets = 10;

    lachesis::run_worker(&conf).await;

    rt.shutdown_background();

    let db = DbMan::init(&conf.db_conf).await.unwrap();
    let services = db.get_paginated_services(0, 100).await.unwrap();

    assert_eq!(services.rows_count, 2);
    // TODO - Check the other tables
}
