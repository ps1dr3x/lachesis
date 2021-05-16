use colored::Colorize;
use rocket::{self, http::Status, response::NamedFile, Request, State};
use rocket_contrib::json::Json;
use tokio::sync::{mpsc::Sender, Mutex};

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    conf,
    db::{DbMan, PaginatedServices}
};

struct Shared {
    db: DbMan,
    tx: Arc<Mutex<Sender<UIMessage>>>
}

#[derive(Debug, Clone)]
pub struct UIMessage {
    pub message: String,
}

#[get("/")]
async fn home() -> Option<NamedFile> {
    NamedFile::open(Path::new("resources/ui/index.html"))
        .await
        .ok()
}

#[get("/<file..>")]
async fn static_files(file: PathBuf) -> Result<NamedFile, Status> {
    let path = Path::new("resources/ui").join(file);
    NamedFile::open(&path).await.map_err(|_| Status::NotFound)
}

#[get("/services?<offset>&<rows>")]
async fn services(
    state: &State<Shared>,
    offset: i64,
    rows: i64,
) -> Result<Json<PaginatedServices>, Status> {
    match state.db.get_paginated_services(offset, rows).await {
        Ok(ps) => Ok(Json(ps)),
        Err(err) => {
            let msg = UIMessage {
                message: format!("[{}] Db query error: {}", "ERROR".red(), err),
            };
            state.tx.lock().await.send(msg).await.unwrap();
            Err(Status::InternalServerError)
        }
    }
}

#[delete("/services", format = "application/json", data = "<ids>")]
async fn del_services(
    state: &State<Shared>,
    ids: Json<Vec<i64>>,
) -> Result<&str, Status> {
    match state.db.delete_services(ids.to_vec()).await {
        Ok(_ss) => Ok("OK"),
        Err(err) => {
            let msg = UIMessage {
                message: format!("[{}] Db query error: {}", "ERROR".red(), err),
            };
            state.tx.lock().await.send(msg).await.unwrap();
            Err(Status::InternalServerError)
        }
    }
}

#[catch(404)]
fn not_found(_req: &Request) -> &'static str {
    "There’s nothing here. Are you lost?"
}

#[catch(500)]
fn internal_server_error(_req: &Request) -> &'static str {
    "Internal server error :("
}

pub async fn run(tx: Sender<UIMessage>) -> Result<(), rocket::Error> {
    let db_conf = match conf::load_db_conf() {
        Ok(db_conf) => db_conf,
        Err(err) => {
            panic!("[{}] Db conf file error: {}",
                "ERROR".red(),
                err
            );
        }
    };

    let db = match DbMan::init(&db_conf).await {
        Ok(db) => db,
        Err(err) => {
            panic!("[{}] Db initialization/connection error: {}",
                "ERROR".red(),
                err
            );
        }
    };

    rocket::build()
        .mount("/", routes![home, static_files])
        .mount("/api", routes![services, del_services])
        .manage(Shared {
            db,
            tx: Arc::new(Mutex::new(tx))
        })
        .register("/", catchers![internal_server_error, not_found])
        .ignite()
        .await?
        .launch()
        .await
}
