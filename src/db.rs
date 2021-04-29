use std::path::Path;

use rusqlite::{types::ToSql, Connection, Error};
use serde_derive::{Deserialize, Serialize};

use crate::detector::DetectorResponse;

#[derive(Serialize, Deserialize, Debug)]
struct ServicesRow {
    pub id: u32,
    pub first_seen: String,
    pub service: String,
    pub version: String,
    pub description: String,
    pub protocol: String,
    pub ip: String,
    pub domain: String,
    pub port: u16,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PaginatedServices {
    services: Vec<ServicesRow>,
    pub rows_count: u32,
}

pub struct DbMan {
    conn: Connection,
}

impl DbMan {
    pub fn init(path: Option<String>) -> Result<Self, Error> {
        let path = path.unwrap_or_else(|| "data/db/services".to_string());
        let conn = Connection::open(Path::new(&path))?;

        conn.execute(
            "
            CREATE TABLE IF NOT EXISTS services (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                first_seen      DATETIME DEFAULT CURRENT_TIMESTAMP,
                service         TEXT,
                version         TEXT,
                description     TEXT NOT NULL,
                protocol        TEXT NOT NULL,
                ip              TEXT NOT NULL,
                domain          TEXT NOT NULL,
                port            INTEGER NOT NULL
            )
        ",
            [],
        )?;

        Ok(DbMan { conn })
    }

    pub fn save_service(&self, service: &DetectorResponse) -> Result<i64, Error> {
        self.conn
            .prepare(
                "
            INSERT INTO services (service, version, description, protocol, ip, domain, port)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            )?
            .insert(&[
                &service.service,
                &service.version,
                &service.description,
                &service.target.protocol,
                &service.target.ip,
                &service.target.domain,
                &service.target.port as &dyn ToSql,
            ])
    }

    pub fn get_paginated_services(
        &self,
        offset: u32,
        rows: u32,
    ) -> Result<PaginatedServices, Error> {
        let mut qy = self.conn.prepare(
            "
            SELECT *
            FROM services
            ORDER BY id DESC
            LIMIT ?
            OFFSET ?
        ",
        )?;

        let services_iter = qy.query_map(&[&rows, &offset], |row| {
            Ok(ServicesRow {
                id: row.get(0)?,
                first_seen: row.get(1)?,
                service: row.get(2)?,
                version: row.get(3)?,
                description: row.get(4)?,
                protocol: row.get(5)?,
                ip: row.get(6)?,
                domain: row.get(7)?,
                port: row.get(8)?,
            })
        })?;

        let mut services_vec = Vec::new();
        for service in services_iter {
            services_vec.push(service?);
        }

        let rows_count = self
            .conn
            .query_row("SELECT COUNT(*) FROM services", [], |row| row.get(0))?;

        Ok(PaginatedServices {
            services: services_vec,
            rows_count,
        })
    }

    pub fn delete_services(&self, ids: Vec<u32>) -> Result<(), Error> {
        for n in &ids {
            self.conn
                .execute("DELETE FROM services WHERE id = ?", &[n])?;
        }
        Ok(())
    }
}
