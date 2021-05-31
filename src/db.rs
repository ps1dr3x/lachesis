use std::time::{SystemTime, UNIX_EPOCH};

use colored::Colorize;
use serde_derive::{Deserialize, Serialize};
use tokio_postgres::{connect, Client, Error, NoTls};

use crate::{conf::DbConf, detector::DetectorResponse};

#[derive(Serialize, Deserialize, Debug)]
struct ServicesRow {
    pub id: i64,
    pub first_seen: u128,
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
    pub rows_count: i64,
}

pub struct DbMan {
    client: Client,
}

impl DbMan {
    pub async fn init(db_conf: &DbConf) -> Result<Self, Error> {
        let (client, connection) = connect(
            &format!(
                "host={} port={} dbname={} user={} password={}",
                db_conf.host, db_conf.port, db_conf.dbname, db_conf.user, db_conf.password
            ),
            NoTls,
        )
        .await?;

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                panic!("[{}] DB connection error: {}", "ERROR".red(), e);
            }
        });

        client
            .simple_query(
                "
                CREATE TABLE IF NOT EXISTS domains (
                    id              bigserial PRIMARY KEY,
                    first_seen      timestamp DEFAULT current_timestamp,
                    last_seen       timestamp DEFAULT current_timestamp,
                    seen_count      integer DEFAULT 1,
                    domain          varchar(1000) UNIQUE NOT NULL
                );

                CREATE TABLE IF NOT EXISTS ips_ports (
                    id              bigserial PRIMARY KEY,
                    first_seen      timestamp DEFAULT current_timestamp,
                    last_seen       timestamp DEFAULT current_timestamp,
                    seen_count      integer DEFAULT 1,
                    ip              varchar(100) UNIQUE NOT NULL,
                    ports           integer[]
                );

                CREATE TABLE IF NOT EXISTS ips_domains (
                    id              bigserial PRIMARY KEY,
                    first_seen      timestamp DEFAULT current_timestamp,
                    last_seen       timestamp DEFAULT current_timestamp,
                    seen_count      integer DEFAULT 1,
                    ip_id           bigserial REFERENCES ips_ports(id),
                    domain_id       bigserial REFERENCES domains(id)
                );

                CREATE TABLE IF NOT EXISTS services (
                    id              bigserial PRIMARY KEY,
                    first_seen      timestamp DEFAULT current_timestamp,
                    last_seen       timestamp DEFAULT current_timestamp,
                    seen_count      integer DEFAULT 1,
                    service         varchar(1000) NOT NULL,
                    version         varchar(1000),
                    description     varchar(1000),
                    protocol        varchar(100) NOT NULL,
                    ip_id           bigserial REFERENCES ips_ports(id) NOT NULL,
                    domain          varchar(1000),
                    port            integer NOT NULL,
                    UNIQUE          (service, ip_id, port)
                );

                --
                -- Trigger that updates the last_seen field at every row update
                --
                CREATE OR REPLACE FUNCTION last_seen_trigger() RETURNS trigger
                LANGUAGE plpgsql AS
                $$BEGIN
                    NEW.last_seen := current_timestamp;
                    RETURN NEW;
                END;$$;

                DROP TRIGGER IF EXISTS last_seen_trigger ON domains;

                CREATE TRIGGER last_seen_trigger
                BEFORE UPDATE ON domains
                FOR EACH ROW
                EXECUTE PROCEDURE last_seen_trigger();

                DROP TRIGGER IF EXISTS last_seen_trigger ON ips_ports;

                CREATE TRIGGER last_seen_trigger
                BEFORE UPDATE ON ips_ports
                FOR EACH ROW
                EXECUTE PROCEDURE last_seen_trigger();

                DROP TRIGGER IF EXISTS last_seen_trigger ON ips_domains;

                CREATE TRIGGER last_seen_trigger
                BEFORE UPDATE ON ips_domains
                FOR EACH ROW
                EXECUTE PROCEDURE last_seen_trigger();

                DROP TRIGGER IF EXISTS last_seen_trigger ON services;

                CREATE TRIGGER last_seen_trigger
                BEFORE UPDATE ON services
                FOR EACH ROW
                EXECUTE PROCEDURE last_seen_trigger();

                --
                -- Trigger that increments the seen_count field at every row update
                --
                CREATE OR REPLACE FUNCTION seen_count_trigger() RETURNS trigger
                LANGUAGE plpgsql AS
                $$BEGIN
                    NEW.seen_count := OLD.seen_count + 1;
                    RETURN NEW;
                END;$$;

                DROP TRIGGER IF EXISTS seen_count_trigger ON domains;

                CREATE TRIGGER seen_count_trigger
                BEFORE UPDATE ON domains
                FOR EACH ROW
                EXECUTE PROCEDURE seen_count_trigger();

                DROP TRIGGER IF EXISTS seen_count_trigger ON ips_ports;

                CREATE TRIGGER seen_count_trigger
                BEFORE UPDATE ON ips_ports
                FOR EACH ROW
                EXECUTE PROCEDURE seen_count_trigger();

                DROP TRIGGER IF EXISTS seen_count_trigger ON ips_domains;

                CREATE TRIGGER seen_count_trigger
                BEFORE UPDATE ON ips_domains
                FOR EACH ROW
                EXECUTE PROCEDURE seen_count_trigger();

                DROP TRIGGER IF EXISTS seen_count_trigger ON services;

                CREATE TRIGGER seen_count_trigger
                BEFORE UPDATE ON services
                FOR EACH ROW
                EXECUTE PROCEDURE seen_count_trigger();
            ",
            )
            .await?;

        Ok(DbMan { client })
    }

    pub async fn update_or_insert_ip(&self, ip: &str) -> Result<i64, Error> {
        let stmt = self
            .client
            .prepare(
                "
                INSERT INTO ips_ports (ip)
                VALUES ($1)
                ON CONFLICT (ip) DO UPDATE
                -- Workaround: do nothing but trigger the update triggers
                SET ip = excluded.ip
                RETURNING id
            ",
            )
            .await?;
        let res = self.client.query_one(&stmt, &[&ip]).await?;

        Ok(res.get(0))
    }

    pub async fn update_or_insert_ip_ports(&self, ip: &str, ports: Vec<u16>) -> Result<i64, Error> {
        let ports: Vec<i32> = ports.iter().map(|port| *port as i32).collect();

        let stmt = self
            .client
            .prepare(
                "
                INSERT INTO ips_ports (ip, ports)
                VALUES ($1, $2)
                ON CONFLICT (ip) DO UPDATE
                SET ports = excluded.ports
                RETURNING id
            ",
            )
            .await?;
        let res = self.client.query_one(&stmt, &[&ip, &ports]).await?;

        Ok(res.get(0))
    }

    pub async fn update_or_insert_domain(&self, domain: &str) -> Result<i64, Error> {
        let stmt = self
            .client
            .prepare(
                "
                INSERT INTO domains (domain)
                VALUES ($1)
                ON CONFLICT (domain) DO UPDATE
                -- Workaround: do nothing but trigger the update triggers
                SET domain = excluded.domain
                RETURNING id
            ",
            )
            .await?;
        let res = self.client.query_one(&stmt, &[&domain]).await?;

        Ok(res.get(0))
    }

    pub async fn save_service(&self, service: &DetectorResponse) -> Result<u64, Error> {
        let ip_id = self.update_or_insert_ip(&service.target.ip).await?;

        let stmt = self
            .client
            .prepare(
                "
                INSERT INTO services (service, version, description, protocol, ip_id, domain, port)
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                ON CONFLICT (service, ip_id, port) DO UPDATE
                -- Workaround: do nothing but trigger the update triggers
                SET ip_id = excluded.ip_id
            ",
            )
            .await?;
        self.client
            .execute(
                &stmt,
                &[
                    &service.service,
                    &service.version,
                    &service.description,
                    &service.target.protocol,
                    &ip_id,
                    &service.target.domain,
                    &(service.target.port as i32),
                ],
            )
            .await
    }

    pub async fn get_paginated_services(
        &self,
        offset: i64,
        rows: i64,
    ) -> Result<PaginatedServices, Error> {
        let stmt = self
            .client
            .prepare(
                "
                SELECT services.id,
                    services.first_seen,
                    services.service,
                    services.version,
                    services.description,
                    services.protocol,
                    ips_ports.ip,
                    services.domain,
                    services.port
                FROM services
                LEFT JOIN ips_ports ON services.ip_id = ips_ports.id
                ORDER BY first_seen DESC
                LIMIT $1
                OFFSET $2
            ",
            )
            .await?;

        let services = self.client.query(&stmt, &[&rows, &offset]).await?;
        let services = services.iter().map(|row| {
            Ok(ServicesRow {
                id: row.get(0),
                first_seen: row
                    .get::<_, SystemTime>(1)
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis(),
                service: row.get(2),
                version: row.get(3),
                description: row.get(4),
                protocol: row.get(5),
                ip: row.get(6),
                domain: row.get(7),
                port: row.get::<_, i32>(8) as u16,
            })
        });

        let mut services_vec = Vec::new();
        for service in services {
            services_vec.push(service?);
        }

        let rows_count = self
            .client
            .query_one("SELECT COUNT(*) FROM services", &[])
            .await?
            .get(0);

        Ok(PaginatedServices {
            services: services_vec,
            rows_count,
        })
    }

    pub async fn delete_services(&self, ids: Vec<i64>) -> Result<(), Error> {
        for n in &ids {
            self.client
                .query("DELETE FROM services WHERE id = $1", &[n])
                .await?;
        }
        Ok(())
    }
}
