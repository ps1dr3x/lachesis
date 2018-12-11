use std::path::Path;
use rusqlite::{
    Connection,
    Error,
    NO_PARAMS,
    types::ToSql
};
use crate::detector::DetectorResponse;

#[derive(Debug)]
pub struct ServicesResult {
    pub id: u32,
    pub time_created: String,
    pub service: String,
    pub version: String,
    pub description: String,
    pub host: String,
    pub port: u16
}

pub struct DbMan {
    conn: Connection
}

impl DbMan {
    pub fn init() -> Result<Self, Error> {
        let conn = Connection::open(Path::new("db/service"))?;

        conn.execute("
            CREATE TABLE IF NOT EXISTS services (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                time_created    DATETIME DEFAULT CURRENT_TIMESTAMP,
                service         TEXT,
                version         TEXT,
                description     TEXT NOT NULL,
                host            TEXT NOT NULL,
                port            INTEGER NOT NULL
            )
        ", NO_PARAMS)?;

        Ok(DbMan { conn })
    }

    pub fn save_service(&self, service: &DetectorResponse) -> Result<usize, Error> {
        self.conn.execute("
            INSERT INTO services (service, version, description, host, port)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ", &[
                &service.service,
                &service.version,
                &service.description,
                &service.host,
                &service.port as &ToSql
            ]
        )
    }

    pub fn get_all_services(&self) -> Result<Vec<ServicesResult>, Error> {
        let mut qy = self.conn.prepare("
            SELECT id,
                time_created,
                service,
                version,
                description,
                host,
                port
            FROM services
        ")?;

        let services_iter = qy.query_map(NO_PARAMS, |row| {
            ServicesResult {
                id: row.get(0),
                time_created: row.get(1),
                service: row.get(2),
                version: row.get(3),
                description: row.get(4),
                host: row.get(5),
                port: row.get(6)
            }
        })?;

        let mut services_vec = Vec::new();
        for service in services_iter {
            services_vec.push(service?);
        }

        Ok(services_vec)
    }
}
