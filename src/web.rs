use std::{
    path::{
        Path,
        PathBuf
    },
    sync::{
        mpsc,
        Arc,
        Mutex
    }
};
use rocket::{
    self,
    Request,
    State,
    request::Form,
    response::NamedFile,
    http::Status
};
use rocket_contrib::json::Json;
use colored::Colorize;
use crate::db::{
    DbMan,
    PaginatedServices
};

#[derive(Debug, Clone)]
pub struct UIMessage {
    pub message: String
}

#[derive(FromForm)]
struct Pagination {
    offset: u32,
    rows: u32
}

#[get("/")]
fn home() -> Option<NamedFile> {
    NamedFile::open(Path::new("resources/ui/index.html")).ok()
}

#[get("/<file..>")]
fn static_files(file: PathBuf) -> Result<NamedFile, Status> {
    let path = Path::new("resources/ui").join(file);
    NamedFile::open(&path).map_err(|_| Status::NotFound)
}

#[get("/services?<params..>")]
fn services(tx: State<Arc<Mutex<mpsc::Sender<UIMessage>>>>, params: Form<Pagination>) -> Result<Json<PaginatedServices>, Status> {
    let db = match DbMan::init() {
        Ok(db) => db,
        Err(err) => {
            let msg = UIMessage {
                message: format!(
                    "[{}] Embedded db initialization/connection error: {}",
                    "ERROR".red(),
                    err
                )
            };
            tx.lock().unwrap().send(msg).unwrap();
            return Err(Status::InternalServerError)
        }
    };

    match db.get_paginated_services(params.offset, params.rows) {
        Ok(ss) => Ok(Json(ss)),
        Err(err) => {
            let msg = UIMessage {
                message: format!(
                    "[{}] Embedded db query error: {}",
                    "ERROR".red(),
                    err
                )
            };
            tx.lock().unwrap().send(msg).unwrap();
            Err(Status::InternalServerError)
        }
    }
}

#[delete("/services", format = "application/json", data = "<ids>")]
fn del_services(tx: State<Arc<Mutex<mpsc::Sender<UIMessage>>>>, ids: Json<Vec<u32>>) -> Result<&str, Status> {
    let db = match DbMan::init() {
        Ok(db) => db,
        Err(err) => {
            let msg = UIMessage {
                message: format!(
                    "[{}] Embedded db initialization/connection error: {}",
                    "ERROR".red(),
                    err
                )
            };
            tx.lock().unwrap().send(msg).unwrap();
            return Err(Status::InternalServerError)
        }
    };

    match db.delete_services(ids.to_vec()) {
        Ok(_ss) => Ok("OK"),
        Err(err) => {
            let msg = UIMessage {
                message: format!(
                    "[{}] Embedded db query error: {}",
                    "ERROR".red(),
                    err
                )
            };
            tx.lock().unwrap().send(msg).unwrap();
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

pub fn run(tx: mpsc::Sender<UIMessage>) {
    rocket::ignite()
        .mount("/", routes![home, static_files])
        .mount("/api", routes![services, del_services])
        .manage(Arc::new(Mutex::new(tx)))
        .register(catchers![internal_server_error, not_found])
        .launch();
}