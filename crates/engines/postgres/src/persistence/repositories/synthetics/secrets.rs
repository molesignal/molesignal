// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::Row;

use super::{
    PgSyntheticRepository,
    codec::{SECRET_COLS, row_to_secret},
};
use crate::{
    domain::synthetics::{
        SecretMaterial, SyntheticSecret, SyntheticSecretRepository, SyntheticSecretVersion,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

fn seal(
    repository: &PgSyntheticRepository,
    material: &SecretMaterial,
) -> Result<(Vec<u8>, Vec<u8>)> {
    if material.0.is_empty() || material.0.len() > 64 * 1024 {
        return Err(Error::invalid(
            "Synthetic Secret material must contain 1 to 65536 bytes",
        ));
    }
    repository
        .cipher
        .seal(&material.0)
        .map_err(|error| Error::internal(format!("encrypt Synthetic Secret: {error}")))
}

#[async_trait]
impl SyntheticSecretRepository for PgSyntheticRepository {
    async fn create_secret(
        &self,
        secret: SyntheticSecret,
        material: SecretMaterial,
    ) -> Result<SyntheticSecret> {
        if secret.current_version != 1 {
            return Err(Error::invalid(
                "new Synthetic Secrets must start at version 1",
            ));
        }
        let (nonce, ciphertext) = seal(self, &material)?;
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let sql = format!(
            "INSERT INTO synthetic_secrets
                (id, organization_id, name, description, current_version, created_by,
                 created_at_micros, updated_at_micros, archived_at_micros)
             VALUES ($1, $2, $3, $4, 1, $5, $6, $7, NULL)
             RETURNING {SECRET_COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(&secret.id.0)
            .bind(&secret.organization_id.0)
            .bind(&secret.name)
            .bind(&secret.description)
            .bind(&secret.created_by.0)
            .bind(secret.created_at.0)
            .bind(secret.updated_at.0)
            .fetch_one(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;
        sqlx::query(
            "INSERT INTO synthetic_secret_versions
                (secret_id, organization_id, version, nonce, ciphertext, created_by,
                 created_at_micros)
             VALUES ($1, $2, 1, $3, $4, $5, $6)",
        )
        .bind(&secret.id.0)
        .bind(&secret.organization_id.0)
        .bind(nonce)
        .bind(ciphertext)
        .bind(&secret.created_by.0)
        .bind(secret.created_at.0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        let secret = row_to_secret(row)?;
        transaction.commit().await.map_err(super::sqlx_err)?;
        Ok(secret)
    }

    async fn rotate_secret(
        &self,
        version: SyntheticSecretVersion,
        material: SecretMaterial,
    ) -> Result<SyntheticSecret> {
        let (nonce, ciphertext) = seal(self, &material)?;
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let current: i32 = sqlx::query_scalar(
            "SELECT current_version FROM synthetic_secrets
             WHERE organization_id = $1 AND id = $2 AND archived_at_micros IS NULL
             FOR UPDATE",
        )
        .bind(&version.organization_id.0)
        .bind(&version.secret_id.0)
        .fetch_one(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        if version.version != current as u32 + 1 {
            return Err(Error::conflict(format!(
                "Synthetic Secret next version is {}, received {}",
                current + 1,
                version.version
            )));
        }
        sqlx::query(
            "INSERT INTO synthetic_secret_versions
                (secret_id, organization_id, version, nonce, ciphertext, created_by,
                 created_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&version.secret_id.0)
        .bind(&version.organization_id.0)
        .bind(version.version as i32)
        .bind(nonce)
        .bind(ciphertext)
        .bind(&version.created_by.0)
        .bind(version.created_at.0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        let sql = format!(
            "UPDATE synthetic_secrets
             SET current_version = $3, updated_at_micros = $4
             WHERE organization_id = $1 AND id = $2
             RETURNING {SECRET_COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(&version.organization_id.0)
            .bind(&version.secret_id.0)
            .bind(version.version as i32)
            .bind(version.created_at.0)
            .fetch_one(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;
        let secret = row_to_secret(row)?;
        transaction.commit().await.map_err(super::sqlx_err)?;
        Ok(secret)
    }

    async fn resolve_secret(
        &self,
        org_id: &Id,
        secret_id: &Id,
        version: Option<u32>,
    ) -> Result<(SyntheticSecretVersion, SecretMaterial)> {
        let row = sqlx::query(
            "SELECT version.version, version.nonce, version.ciphertext,
                    version.created_by, version.created_at_micros
             FROM synthetic_secrets secret
             JOIN synthetic_secret_versions version
               ON version.organization_id = secret.organization_id
              AND version.secret_id = secret.id
              AND version.version = COALESCE($3, secret.current_version)
             WHERE secret.organization_id = $1 AND secret.id = $2
               AND secret.archived_at_micros IS NULL",
        )
        .bind(&org_id.0)
        .bind(&secret_id.0)
        .bind(version.map(|value| value as i32))
        .fetch_one(&self.pool)
        .await
        .map_err(super::sqlx_err)?;
        let nonce: Vec<u8> = row.try_get("nonce").map_err(super::sqlx_err)?;
        let ciphertext: Vec<u8> = row.try_get("ciphertext").map_err(super::sqlx_err)?;
        let plaintext = self
            .cipher
            .open(&nonce, &ciphertext)
            .map_err(|error| Error::internal(format!("decrypt Synthetic Secret: {error}")))?;
        Ok((
            SyntheticSecretVersion {
                secret_id: secret_id.clone(),
                organization_id: org_id.clone(),
                version: row.try_get::<i32, _>("version").map_err(super::sqlx_err)? as u32,
                created_by: Id(row.try_get("created_by").map_err(super::sqlx_err)?),
                created_at: TimestampMicros(
                    row.try_get("created_at_micros").map_err(super::sqlx_err)?,
                ),
            },
            SecretMaterial(plaintext),
        ))
    }

    async fn get_secret(&self, org_id: &Id, secret_id: &Id) -> Result<SyntheticSecret> {
        let sql = format!(
            "SELECT {SECRET_COLS} FROM synthetic_secrets
             WHERE organization_id = $1 AND id = $2"
        );
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&secret_id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;
        row_to_secret(row)
    }

    async fn list_secrets(&self, org_id: &Id) -> Result<Vec<SyntheticSecret>> {
        let sql = format!(
            "SELECT {SECRET_COLS} FROM synthetic_secrets
             WHERE organization_id = $1
             ORDER BY archived_at_micros NULLS FIRST, name, id"
        );
        sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(row_to_secret)
            .collect()
    }
}
