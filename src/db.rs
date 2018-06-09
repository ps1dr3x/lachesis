extern crate rusqlite;

use std::path::Path;
use self::rusqlite::{ Connection, Error };

#[derive(Debug)]
pub struct Vulnerable {
    pub service: String,
    pub version: String,
    pub exploit: String,
    pub host: String,
    pub port: u16
}

#[derive(Debug)]
pub struct VulnerableResult {
    pub id: u32,
    pub time_created: String,
    pub service: String,
    pub version: String,
    pub exploit: String,
    pub host: String,
    pub port: u16
}

pub struct DbMan {
    conn: Connection
}

impl DbMan {
    pub fn new() -> DbMan {
        let conn = Connection::open(Path::new("db/vulnerable")).unwrap();

        conn.execute("
            CREATE TABLE IF NOT EXISTS vulnerable (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                time_created    DATETIME DEFAULT CURRENT_TIMESTAMP,
                service         TEXT,
                version         TEXT,
                exploit         TEXT NOT NULL,
                host            TEXT NOT NULL,
                port            INTEGER NOT NULL
            )
        ", &[]).unwrap();

        DbMan {
            conn
        }
    }

    pub fn save_vuln(&self, vulnerable: Vulnerable) -> Result<i32, Error> {
        self.conn.execute("
            INSERT INTO vulnerable (service, version, exploit, host, port)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ", &[
                &vulnerable.service,
                &vulnerable.version,
                &vulnerable.exploit,
                &vulnerable.host,
                &vulnerable.port
            ]
        )
    }

    pub fn get_all_vuln(&self) -> Result<Vec<VulnerableResult>, Error> {
        let mut qy = self.conn.prepare("
            SELECT id,
                time_created,
                service,
                version,
                exploit,
                host,
                port
            FROM vulnerable
        ").unwrap();

        let vulnerable_iter = qy.query_map(&[], |row| {
            VulnerableResult {
                id: row.get(0),
                time_created: row.get(1),
                service: row.get(2),
                version: row.get(3),
                exploit: row.get(4),
                host: row.get(5),
                port: row.get(6)
            }
        }).unwrap();

        let mut vulnerable_vec = Vec::new();
        for vuln in vulnerable_iter {
            vulnerable_vec.push(vuln.unwrap());
        }
        
        Ok(vulnerable_vec)
    }
}
