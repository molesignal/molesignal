// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PostgreSQL-backed immutable Dashboard contract publications and active binding.

use async_trait::async_trait;
use sqlx::{PgPool, Row, postgres::PgRow, types::Json};

use super::sqlx_err;
use crate::{
    domain::dashboard::contract_registry::{
        DashboardContractBinding, DashboardContractBundle, DashboardContractDocuments,
        DashboardContractKind, DashboardContractRef, DashboardContractRepository,
        DashboardContractSelection, DashboardContractStatus, DashboardContractVersion,
    },
    shared::{Error, Result, time::TimestampMicros},
};

const BINDING_COLUMNS: &str = "capability_key, revision,
    model_contract_key, model_contract_version, model_schema_hash,
    authoring_contract_key, authoring_contract_version, authoring_schema_hash,
    visualization_contract_key, visualization_contract_version, visualization_schema_hash,
    compiler_version, enabled, updated_at_micros";

pub struct PgDashboardContractRepository {
    pool: PgPool,
}

impl PgDashboardContractRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn load_binding(&self, capability_key: &str) -> Result<DashboardContractBinding> {
        let row = sqlx::query(&format!(
            "SELECT {BINDING_COLUMNS}
             FROM intelligence_capability_contract_bindings
             WHERE capability_key = $1"
        ))
        .bind(capability_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::unavailable("Dashboard contract binding is not published"))?;
        row_to_binding(&row)
    }

    async fn load_version(
        &self,
        reference: &DashboardContractRef,
    ) -> Result<DashboardContractVersion> {
        let row = sqlx::query(
            "SELECT contract_key, version, kind, dialect, document, schema_hash,
                    status, published_at_micros
             FROM intelligence_contract_versions
             WHERE contract_key = $1 AND version = $2 AND schema_hash = $3",
        )
        .bind(&reference.contract_key)
        .bind(to_i32(reference.version)?)
        .bind(&reference.schema_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::unavailable("Dashboard contract publication is missing"))?;
        row_to_version(&row, "")
    }
}

#[async_trait]
impl DashboardContractRepository for PgDashboardContractRepository {
    async fn publish_builtin(
        &self,
        versions: &[DashboardContractVersion],
        default_selection: &DashboardContractSelection,
        now: TimestampMicros,
    ) -> Result<DashboardContractBinding> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        for version in versions {
            sqlx::query(
                "INSERT INTO intelligence_contract_versions
                 (contract_key, version, kind, dialect, document, schema_hash, status,
                  published_at_micros)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                 ON CONFLICT (contract_key, version) DO NOTHING",
            )
            .bind(&version.contract_key)
            .bind(to_i32(version.version)?)
            .bind(version.kind.as_str())
            .bind(&version.dialect)
            .bind(Json(&version.document))
            .bind(&version.schema_hash)
            .bind(version.status.as_str())
            .bind(version.published_at.0)
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;

            let stored = sqlx::query(
                "SELECT kind, dialect, document, schema_hash, status
                 FROM intelligence_contract_versions
                 WHERE contract_key = $1 AND version = $2",
            )
            .bind(&version.contract_key)
            .bind(to_i32(version.version)?)
            .fetch_one(&mut *transaction)
            .await
            .map_err(sqlx_err)?;
            let stored_document: Json<serde_json::Value> =
                stored.try_get("document").map_err(sqlx_err)?;
            let matches = stored.try_get::<String, _>("kind").map_err(sqlx_err)?
                == version.kind.as_str()
                && stored.try_get::<String, _>("dialect").map_err(sqlx_err)? == version.dialect
                && stored_document.0 == version.document
                && stored
                    .try_get::<String, _>("schema_hash")
                    .map_err(sqlx_err)?
                    == version.schema_hash
                && stored.try_get::<String, _>("status").map_err(sqlx_err)?
                    == version.status.as_str();
            if !matches {
                return Err(Error::conflict(format!(
                    "Dashboard contract {} version {} is immutable and differs from the deployed publication",
                    version.contract_key, version.version
                )));
            }
        }

        insert_default_binding(&mut transaction, default_selection, now).await?;
        transaction.commit().await.map_err(sqlx_err)?;
        self.load_binding(&default_selection.capability_key).await
    }

    async fn load_active(&self, capability_key: &str) -> Result<DashboardContractBundle> {
        let row = sqlx::query(&format!(
            "SELECT {BINDING_COLUMNS},
               m.contract_key AS model_version_contract_key,
               m.version AS model_version_version, m.kind AS model_version_kind,
               m.dialect AS model_version_dialect, m.document AS model_version_document,
               m.schema_hash AS model_version_schema_hash, m.status AS model_version_status,
               m.published_at_micros AS model_version_published_at_micros,
               a.contract_key AS authoring_version_contract_key,
               a.version AS authoring_version_version, a.kind AS authoring_version_kind,
               a.dialect AS authoring_version_dialect,
               a.document AS authoring_version_document,
               a.schema_hash AS authoring_version_schema_hash,
               a.status AS authoring_version_status,
               a.published_at_micros AS authoring_version_published_at_micros,
               v.contract_key AS visualization_version_contract_key,
               v.version AS visualization_version_version,
               v.kind AS visualization_version_kind, v.dialect AS visualization_version_dialect,
               v.document AS visualization_version_document,
               v.schema_hash AS visualization_version_schema_hash,
               v.status AS visualization_version_status,
               v.published_at_micros AS visualization_version_published_at_micros
             FROM intelligence_capability_contract_bindings b
             JOIN intelligence_contract_versions m
               ON (m.contract_key, m.version, m.schema_hash) =
                  (b.model_contract_key, b.model_contract_version, b.model_schema_hash)
             JOIN intelligence_contract_versions a
               ON (a.contract_key, a.version, a.schema_hash) =
                  (b.authoring_contract_key, b.authoring_contract_version,
                   b.authoring_schema_hash)
             JOIN intelligence_contract_versions v
               ON (v.contract_key, v.version, v.schema_hash) =
                  (b.visualization_contract_key, b.visualization_contract_version,
                   b.visualization_schema_hash)
             WHERE b.capability_key = $1"
        ))
        .bind(capability_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::unavailable("Dashboard contract binding is not published"))?;
        let binding = row_to_binding(&row)?;
        let documents = DashboardContractDocuments {
            model: row_to_version(&row, "model_version_")?,
            authoring: row_to_version(&row, "authoring_version_")?,
            visualization: row_to_version(&row, "visualization_version_")?,
        };
        Ok(DashboardContractBundle { binding, documents })
    }

    async fn load_documents(
        &self,
        selection: &DashboardContractSelection,
    ) -> Result<DashboardContractDocuments> {
        Ok(DashboardContractDocuments {
            model: self.load_version(&selection.model).await?,
            authoring: self.load_version(&selection.authoring).await?,
            visualization: self.load_version(&selection.visualization).await?,
        })
    }

    async fn activate(
        &self,
        selection: &DashboardContractSelection,
        now: TimestampMicros,
    ) -> Result<DashboardContractBinding> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let row = sqlx::query(&format!(
            "INSERT INTO intelligence_capability_contract_bindings
             ({BINDING_COLUMNS})
             VALUES ($1, 1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
             ON CONFLICT (capability_key) DO UPDATE SET
               revision = intelligence_capability_contract_bindings.revision + 1,
               model_contract_key = EXCLUDED.model_contract_key,
               model_contract_version = EXCLUDED.model_contract_version,
               model_schema_hash = EXCLUDED.model_schema_hash,
               authoring_contract_key = EXCLUDED.authoring_contract_key,
               authoring_contract_version = EXCLUDED.authoring_contract_version,
               authoring_schema_hash = EXCLUDED.authoring_schema_hash,
               visualization_contract_key = EXCLUDED.visualization_contract_key,
               visualization_contract_version = EXCLUDED.visualization_contract_version,
               visualization_schema_hash = EXCLUDED.visualization_schema_hash,
               compiler_version = EXCLUDED.compiler_version,
               enabled = EXCLUDED.enabled,
               updated_at_micros = EXCLUDED.updated_at_micros
             RETURNING {BINDING_COLUMNS}"
        ))
        .bind(&selection.capability_key)
        .bind(&selection.model.contract_key)
        .bind(to_i32(selection.model.version)?)
        .bind(&selection.model.schema_hash)
        .bind(&selection.authoring.contract_key)
        .bind(to_i32(selection.authoring.version)?)
        .bind(&selection.authoring.schema_hash)
        .bind(&selection.visualization.contract_key)
        .bind(to_i32(selection.visualization.version)?)
        .bind(&selection.visualization.schema_hash)
        .bind(&selection.compiler_version)
        .bind(selection.enabled)
        .bind(now.0)
        .fetch_one(&mut *transaction)
        .await
        .map_err(sqlx_err)?;
        let binding = row_to_binding(&row)?;
        transaction.commit().await.map_err(sqlx_err)?;
        Ok(binding)
    }
}

async fn insert_default_binding(
    transaction: &mut sqlx::PgConnection,
    selection: &DashboardContractSelection,
    now: TimestampMicros,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO intelligence_capability_contract_bindings
         (capability_key, revision,
          model_contract_key, model_contract_version, model_schema_hash,
          authoring_contract_key, authoring_contract_version, authoring_schema_hash,
          visualization_contract_key, visualization_contract_version,
          visualization_schema_hash, compiler_version, enabled, updated_at_micros)
         VALUES ($1, 1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
         ON CONFLICT (capability_key) DO NOTHING",
    )
    .bind(&selection.capability_key)
    .bind(&selection.model.contract_key)
    .bind(to_i32(selection.model.version)?)
    .bind(&selection.model.schema_hash)
    .bind(&selection.authoring.contract_key)
    .bind(to_i32(selection.authoring.version)?)
    .bind(&selection.authoring.schema_hash)
    .bind(&selection.visualization.contract_key)
    .bind(to_i32(selection.visualization.version)?)
    .bind(&selection.visualization.schema_hash)
    .bind(&selection.compiler_version)
    .bind(selection.enabled)
    .bind(now.0)
    .execute(transaction)
    .await
    .map_err(sqlx_err)?;
    Ok(())
}

fn row_to_binding(row: &PgRow) -> Result<DashboardContractBinding> {
    Ok(DashboardContractBinding {
        selection: DashboardContractSelection {
            capability_key: row.try_get("capability_key").map_err(sqlx_err)?,
            model: contract_ref(row, "model")?,
            authoring: contract_ref(row, "authoring")?,
            visualization: contract_ref(row, "visualization")?,
            compiler_version: row.try_get("compiler_version").map_err(sqlx_err)?,
            enabled: row.try_get("enabled").map_err(sqlx_err)?,
        },
        revision: row.try_get("revision").map_err(sqlx_err)?,
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

fn contract_ref(row: &PgRow, prefix: &str) -> Result<DashboardContractRef> {
    let version: i32 = row
        .try_get(format!("{prefix}_contract_version").as_str())
        .map_err(sqlx_err)?;
    Ok(DashboardContractRef {
        contract_key: row
            .try_get(format!("{prefix}_contract_key").as_str())
            .map_err(sqlx_err)?,
        version: to_u32(version)?,
        schema_hash: row
            .try_get(format!("{prefix}_schema_hash").as_str())
            .map_err(sqlx_err)?,
    })
}

fn row_to_version(row: &PgRow, prefix: &str) -> Result<DashboardContractVersion> {
    let column = |name: &str| format!("{prefix}{name}");
    let kind: String = row.try_get(column("kind").as_str()).map_err(sqlx_err)?;
    let status: String = row.try_get(column("status").as_str()).map_err(sqlx_err)?;
    let version: i32 = row.try_get(column("version").as_str()).map_err(sqlx_err)?;
    let document: Json<serde_json::Value> =
        row.try_get(column("document").as_str()).map_err(sqlx_err)?;
    Ok(DashboardContractVersion {
        contract_key: row
            .try_get(column("contract_key").as_str())
            .map_err(sqlx_err)?,
        version: to_u32(version)?,
        kind: DashboardContractKind::parse(&kind)
            .ok_or_else(|| Error::internal("unknown Dashboard contract kind"))?,
        dialect: row.try_get(column("dialect").as_str()).map_err(sqlx_err)?,
        document: document.0,
        schema_hash: row
            .try_get(column("schema_hash").as_str())
            .map_err(sqlx_err)?,
        status: DashboardContractStatus::parse(&status)
            .ok_or_else(|| Error::internal("unknown Dashboard contract status"))?,
        published_at: TimestampMicros(
            row.try_get(column("published_at_micros").as_str())
                .map_err(sqlx_err)?,
        ),
    })
}

fn to_i32(value: u32) -> Result<i32> {
    i32::try_from(value).map_err(|_| Error::invalid("Dashboard contract version is too large"))
}

fn to_u32(value: i32) -> Result<u32> {
    u32::try_from(value).map_err(|_| Error::internal("invalid Dashboard contract version"))
}
