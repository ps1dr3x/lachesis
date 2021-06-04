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
            .batch_execute(
                "
                CREATE TABLE IF NOT EXISTS domain (
                    id              bigserial PRIMARY KEY,
                    first_seen      timestamp DEFAULT current_timestamp,
                    last_seen       timestamp DEFAULT current_timestamp,
                    seen_count      integer DEFAULT 1,
                    domain          varchar(1000) UNIQUE NOT NULL
                );

                CREATE TABLE IF NOT EXISTS ip_ports (
                    id              bigserial PRIMARY KEY,
                    first_seen      timestamp DEFAULT current_timestamp,
                    last_seen       timestamp DEFAULT current_timestamp,
                    seen_count      integer DEFAULT 1,
                    ip              varchar(100) UNIQUE NOT NULL,
                    ports           integer[]
                );

                CREATE TABLE IF NOT EXISTS ip_domain (
                    id              bigserial PRIMARY KEY,
                    first_seen      timestamp DEFAULT current_timestamp,
                    last_seen       timestamp DEFAULT current_timestamp,
                    seen_count      integer DEFAULT 1,
                    ip_id           bigserial REFERENCES ip_ports(id),
                    domain_id       bigserial REFERENCES domain(id),
                    UNIQUE(ip_id, domain_id)
                );

                CREATE TABLE IF NOT EXISTS service (
                    id              bigserial PRIMARY KEY,
                    first_seen      timestamp DEFAULT current_timestamp,
                    last_seen       timestamp DEFAULT current_timestamp,
                    seen_count      integer DEFAULT 1,
                    service         varchar(1000) NOT NULL,
                    version         varchar(1000),
                    description     varchar(1000),
                    protocol        varchar(100) NOT NULL,
                    ip_id           bigserial REFERENCES ip_ports(id) NOT NULL,
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

                DROP TRIGGER IF EXISTS last_seen_trigger ON domain;

                CREATE TRIGGER last_seen_trigger
                BEFORE UPDATE ON domain
                FOR EACH ROW
                EXECUTE PROCEDURE last_seen_trigger();

                DROP TRIGGER IF EXISTS last_seen_trigger ON ip_ports;

                CREATE TRIGGER last_seen_trigger
                BEFORE UPDATE ON ip_ports
                FOR EACH ROW
                EXECUTE PROCEDURE last_seen_trigger();

                DROP TRIGGER IF EXISTS last_seen_trigger ON ip_domain;

                CREATE TRIGGER last_seen_trigger
                BEFORE UPDATE ON ip_domain
                FOR EACH ROW
                EXECUTE PROCEDURE last_seen_trigger();

                DROP TRIGGER IF EXISTS last_seen_trigger ON service;

                CREATE TRIGGER last_seen_trigger
                BEFORE UPDATE ON service
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

                DROP TRIGGER IF EXISTS seen_count_trigger ON domain;

                CREATE TRIGGER seen_count_trigger
                BEFORE UPDATE ON domain
                FOR EACH ROW
                EXECUTE PROCEDURE seen_count_trigger();

                DROP TRIGGER IF EXISTS seen_count_trigger ON ip_ports;

                CREATE TRIGGER seen_count_trigger
                BEFORE UPDATE ON ip_ports
                FOR EACH ROW
                EXECUTE PROCEDURE seen_count_trigger();

                DROP TRIGGER IF EXISTS seen_count_trigger ON ip_domain;

                CREATE TRIGGER seen_count_trigger
                BEFORE UPDATE ON ip_domain
                FOR EACH ROW
                EXECUTE PROCEDURE seen_count_trigger();

                DROP TRIGGER IF EXISTS seen_count_trigger ON service;

                CREATE TRIGGER seen_count_trigger
                BEFORE UPDATE ON service
                FOR EACH ROW
                EXECUTE PROCEDURE seen_count_trigger();
            ",
            )
            .await?;

        Ok(DbMan { client })
    }

    async fn insert_ip_port(&self, ip: &str, port: u16) -> Result<i64, Error> {
        let port = port as i32; // postgres type

        // If the ip is not in the table yet, insert it with a new array containing this port
        // Else if the port was already detected for this ip, do nothing but trigger the update triggers
        // Else append the port to the existing array
        let stmt = self
            .client
            .prepare(
                "
                INSERT INTO ip_ports (ip, ports)
                VALUES ($1, ARRAY[$2::INTEGER])
                ON CONFLICT (ip)
                DO UPDATE
                SET ports = (
                    CASE
                    WHEN array_position(ip_ports.ports, $2::INTEGER) IS NOT NULL THEN ip_ports.ports
                    ELSE array_append(ip_ports.ports, $2::INTEGER)
                    END
                )
                RETURNING id
            ",
            )
            .await?;

        let res = self.client.query_one(&stmt, &[&ip, &port]).await?;

        Ok(res.get(0))
    }

    async fn update_or_insert_domain(&self, domain: &str) -> Result<i64, Error> {
        let stmt = self
            .client
            .prepare(
                "
                INSERT INTO domain (domain)
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

    async fn update_or_insert_ip_domain_relation(
        &self,
        ip_id: &i64,
        domain_id: &i64,
    ) -> Result<i64, Error> {
        let stmt = self
            .client
            .prepare(
                "
                INSERT INTO ip_domain (ip_id, domain_id)
                VALUES ($1, $2)
                ON CONFLICT (ip_id, domain_id) DO UPDATE
                -- Workaround: do nothing but trigger the update triggers
                SET ip_id = excluded.ip_id
                RETURNING id
            ",
            )
            .await?;
        let res = self.client.query_one(&stmt, &[&ip_id, &domain_id]).await?;

        Ok(res.get(0))
    }

    pub async fn insert_service(&self, service: &DetectorResponse) -> Result<u64, Error> {
        let ip_id = self
            .insert_ip_port(&service.target.ip, service.target.port)
            .await?;

        if !service.target.domain.is_empty() {
            let domain_id = self.update_or_insert_domain(&service.target.domain).await?;
            self.update_or_insert_ip_domain_relation(&ip_id, &domain_id)
                .await?;
        }

        let stmt = self
            .client
            .prepare(
                "
                INSERT INTO service (service, version, description, protocol, ip_id, domain, port)
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
                SELECT service.id,
                    service.first_seen,
                    service.service,
                    service.version,
                    service.description,
                    service.protocol,
                    ip_ports.ip,
                    service.domain,
                    service.port
                FROM service
                LEFT JOIN ip_ports ON service.ip_id = ip_ports.id
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
            .query_one("SELECT COUNT(*) FROM service", &[])
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
                .query("DELETE FROM service WHERE id = $1", &[n])
                .await?;
        }
        Ok(())
    }
}
