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
    response::NamedFile,
    http::Status
};
use rocket_contrib::json::Json;
use colored::Colorize;
use crate::db::{
    DbMan,
    ServicesRow
};

#[derive(Debug, Clone)]
pub struct UIMessage {
    pub message: String
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

#[get("/services")]
fn services(tx: State<Arc<Mutex<mpsc::Sender<UIMessage>>>>) -> Result<Json<Vec<ServicesRow>>, Status> {
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

    match db.get_all_services() {
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
        .mount("/api", routes![services])
        .manage(Arc::new(Mutex::new(tx)))
        .register(catchers![internal_server_error, not_found])
        .launch();
}