// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::{
    domain::notify::{
        connector::{ConnectorCapabilities, ConnectorStatus, ConnectorTestStatus, NotifyConnector},
        repositories::NotifyConnectorRepository,
    },
    infra::cipher::CipherRootKey,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgNotifyConnectorRepository {
    pool: PgPool,
    cipher: CipherRootKey,
}

impl PgNotifyConnectorRepository {
    pub fn new(pool: PgPool, cipher: CipherRootKey) -> Self {
        Self { pool, cipher }
    }

    fn seal_config(&self, connector: &NotifyConnector) -> Result<(Vec<u8>, Vec<u8>)> {
        let plaintext = serde_json::to_vec(&connector.config)
            .map_err(|error| Error::invalid(format!("notify connector config: {error}")))?;
        self.cipher
            .seal(&plaintext)
            .map_err(|error| Error::internal(format!("notify connector config seal: {error}")))
    }

    fn open_config(&self, nonce: &[u8], ciphertext: &[u8]) -> Result<serde_json::Value> {
        let plaintext = self
            .cipher
            .open(nonce, ciphertext)
            .map_err(|_| Error::internal("notify connector config decrypt failed"))?;
        serde_json::from_slice(&plaintext)
            .map_err(|_| Error::internal("notify connector config is not valid JSON"))
    }

    fn row_to_connector(&self, row: sqlx::postgres::PgRow) -> Result<NotifyConnector> {
        let status: String = row.try_get("status").map_err(sqlx_err)?;
        let test_status = row
            .try_get::<Option<String>, _>("last_test_status")
            .map_err(sqlx_err)?
            .map(|value| ConnectorTestStatus::parse(&value))
            .transpose()?;
        let capabilities: Json<ConnectorCapabilities> =
            row.try_get("capabilities").map_err(sqlx_err)?;
        let ciphertext: Vec<u8> = row.try_get("config_ciphertext").map_err(sqlx_err)?;
        let nonce: Vec<u8> = row.try_get("config_nonce").map_err(sqlx_err)?;
        Ok(NotifyConnector {
            id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
            organization_id: Id::from_string(
                row.try_get::<String, _>("organization_id")
                    .map_err(sqlx_err)?,
            ),
            name: row.try_get("name").map_err(sqlx_err)?,
            connector_type: row.try_get("connector_type").map_err(sqlx_err)?,
            config: self.open_config(&nonce, &ciphertext)?,
            capabilities: capabilities.0,
            enabled: row.try_get("enabled").map_err(sqlx_err)?,
            status: ConnectorStatus::parse(&status)?,
            last_tested_at: row
                .try_get::<Option<i64>, _>("last_tested_at_micros")
                .map_err(sqlx_err)?
                .map(TimestampMicros),
            last_test_status: test_status,
            last_test_error: row.try_get("last_test_error").map_err(sqlx_err)?,
            created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
            updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
        })
    }
}

const COLS: &str = "id, organization_id, name, connector_type,
    config_ciphertext, config_nonce, capabilities, enabled, status,
    last_tested_at_micros, last_test_status, last_test_error, created_at_micros,
    updated_at_micros";

#[async_trait]
impl NotifyConnectorRepository for PgNotifyConnectorRepository {
    async fn create(&self, connector: NotifyConnector) -> Result<NotifyConnector> {
        let (nonce, ciphertext) = self.seal_config(&connector)?;
        sqlx::query(
            "INSERT INTO notify_connectors (
                 id, organization_id, name, connector_type,
                 config_ciphertext, config_nonce, capabilities, enabled, status,
                 last_tested_at_micros, last_test_status, last_test_error,
                 created_at_micros, updated_at_micros
             ) VALUES (
                 $1, $2, $3, $4, $5, $6, $7, $8, $9,
                 $10, $11, $12, $13, $14
             )",
        )
        .bind(&connector.id.0)
        .bind(&connector.organization_id.0)
        .bind(&connector.name)
        .bind(&connector.connector_type)
        .bind(&ciphertext)
        .bind(&nonce)
        .bind(Json(&connector.capabilities))
        .bind(connector.enabled)
        .bind(connector.status.as_str())
        .bind(connector.last_tested_at.map(|value| value.0))
        .bind(connector.last_test_status.map(ConnectorTestStatus::as_str))
        .bind(&connector.last_test_error)
        .bind(connector.created_at.0)
        .bind(connector.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(connector)
    }

    async fn update(&self, connector: NotifyConnector) -> Result<NotifyConnector> {
        let (nonce, ciphertext) = self.seal_config(&connector)?;
        let updated = sqlx::query(
            "UPDATE notify_connectors
                SET name = $3,
                    config_ciphertext = $4,
                    config_nonce = $5,
                    capabilities = $6,
                    enabled = $7,
                    status = $8,
                    updated_at_micros = $9
              WHERE organization_id = $1 AND id = $2",
        )
        .bind(&connector.organization_id.0)
        .bind(&connector.id.0)
        .bind(&connector.name)
        .bind(&ciphertext)
        .bind(&nonce)
        .bind(Json(&connector.capabilities))
        .bind(connector.enabled)
        .bind(connector.status.as_str())
        .bind(connector.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        if updated.rows_affected() == 0 {
            return Err(Error::not_found("notify connector"));
        }
        Ok(connector)
    }

    async fn get(&self, organization_id: &Id, id: &Id) -> Result<NotifyConnector> {
        let row = sqlx::query(&format!(
            "SELECT {COLS}
               FROM notify_connectors
              WHERE organization_id = $1 AND id = $2"
        ))
        .bind(&organization_id.0)
        .bind(&id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        self.row_to_connector(row)
    }

    async fn list(&self, organization_id: &Id) -> Result<Vec<NotifyConnector>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS}
               FROM notify_connectors
              WHERE organization_id = $1
           ORDER BY name, id"
        ))
        .bind(&organization_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter()
            .map(|row| self.row_to_connector(row))
            .collect()
    }

    async fn record_test_result(
        &self,
        organization_id: &Id,
        id: &Id,
        tested_at: TimestampMicros,
        status: ConnectorTestStatus,
        error: Option<String>,
    ) -> Result<NotifyConnector> {
        let connector_status = match status {
            ConnectorTestStatus::Success => ConnectorStatus::Connected,
            ConnectorTestStatus::Failed => ConnectorStatus::Error,
        };
        let updated = sqlx::query(
            "UPDATE notify_connectors
                SET status = $3,
                    last_tested_at_micros = $4,
                    last_test_status = $5,
                    last_test_error = $6,
                    updated_at_micros = $4
              WHERE organization_id = $1 AND id = $2",
        )
        .bind(&organization_id.0)
        .bind(&id.0)
        .bind(connector_status.as_str())
        .bind(tested_at.0)
        .bind(status.as_str())
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        if updated.rows_affected() == 0 {
            return Err(Error::not_found("notify connector"));
        }
        self.get(organization_id, id).await
    }

    async fn delete(&self, organization_id: &Id, id: &Id) -> Result<()> {
        let deleted =
            sqlx::query("DELETE FROM notify_connectors WHERE organization_id = $1 AND id = $2")
                .bind(&organization_id.0)
                .bind(&id.0)
                .execute(&self.pool)
                .await
                .map_err(sqlx_err)?;
        if deleted.rows_affected() == 0 {
            return Err(Error::not_found("notify connector"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;

    use super::*;

    fn repository() -> PgNotifyConnectorRepository {
        let key = base64::engine::general_purpose::STANDARD.encode([7u8; 32]);
        let cipher = CipherRootKey::from_base64(&key).unwrap();
        PgNotifyConnectorRepository::new(PgPool::connect_lazy("postgres://unused").unwrap(), cipher)
    }

    #[tokio::test]
    async fn config_envelope_round_trips_without_plaintext_storage() {
        let repo = repository();
        let connector = NotifyConnector {
            id: Id::new(),
            organization_id: Id::new(),
            name: "mail".into(),
            connector_type: "email_smtp".into(),
            config: serde_json::json!({"password": "do-not-leak"}),
            capabilities: ConnectorCapabilities::default(),
            enabled: true,
            status: ConnectorStatus::Unknown,
            last_tested_at: None,
            last_test_status: None,
            last_test_error: None,
            created_at: TimestampMicros::now(),
            updated_at: TimestampMicros::now(),
        };
        let (nonce, ciphertext) = repo.seal_config(&connector).unwrap();
        assert!(!String::from_utf8_lossy(&ciphertext).contains("do-not-leak"));
        assert_eq!(
            repo.open_config(&nonce, &ciphertext).unwrap(),
            connector.config
        );
    }
}
