// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::stream::{
        MOLESIGNAL_SYSTEM_STREAM, Retention, Schema, StreamDefinition, StreamRepository,
        StreamSettings, StreamType,
    },
    infra::caching::OrgSchemaCache,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgStreamRepository {
    pool: PgPool,
    /// 可选 schema 缓存 hook：CRUD 后 invalidate 对应 entry。
    cache: Option<Arc<OrgSchemaCache>>,
}

impl PgStreamRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, cache: None }
    }

    /// 注入 OrgSchemaCache；bootstrap 阶段使用，单测用 `new` 即可。
    pub fn with_cache(mut self, cache: Arc<OrgSchemaCache>) -> Self {
        self.cache = Some(cache);
        self
    }
}

pub(crate) fn stream_type_to_str(t: StreamType) -> &'static str {
    t.as_str()
}

pub(crate) fn stream_type_from_str(s: &str) -> Result<StreamType> {
    match s {
        "logs" => Ok(StreamType::Logs),
        "metrics" => Ok(StreamType::Metrics),
        "traces" => Ok(StreamType::Traces),
        "profiles" => Ok(StreamType::Profiles),
        "extend" => Ok(StreamType::Extend),
        other => Err(Error::internal(format!("unknown stream_type: {other}"))),
    }
}

fn row_to_stream(row: sqlx::postgres::PgRow) -> Result<StreamDefinition> {
    let schema: Json<Schema> = row.try_get("schema").map_err(sqlx_err)?;
    let retention: Option<Json<Retention>> = row.try_get("retention").map_err(sqlx_err)?;
    let stream_type: String = row.try_get("stream_type").map_err(sqlx_err)?;
    Ok(StreamDefinition {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        stream_type: stream_type_from_str(&stream_type)?,
        schema: schema.0,
        retention: retention.map(|value| value.0),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

fn invalidate_stream_cache(
    cache: &Option<Arc<OrgSchemaCache>>,
    org_id: &str,
    name: &str,
    stream_type: &str,
) {
    if let Some(cache) = cache
        && let Ok(stream_type) = stream_type_from_str(stream_type)
    {
        cache.invalidate_for(&Id(org_id.to_string()), name, stream_type);
    }
}

const COLS: &str =
    "id, org_id, name, stream_type, schema, retention, created_at_micros, updated_at_micros";

#[async_trait]
impl StreamRepository for PgStreamRepository {
    async fn create(&self, def: StreamDefinition) -> Result<StreamDefinition> {
        let org_row = sqlx::query("SELECT system FROM organizations WHERE id = $1")
            .bind(&def.org_id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::not_found("organization"))?;
        let org_is_system: bool = org_row.try_get("system").map_err(sqlx_err)?;
        let system = org_is_system && def.name == MOLESIGNAL_SYSTEM_STREAM;
        if (org_is_system || def.name == MOLESIGNAL_SYSTEM_STREAM) && !system {
            return Err(Error::forbidden("invalid system stream identity"));
        }
        let insert = sqlx::query(
            "INSERT INTO streams (id, org_id, name, stream_type, schema, retention, system,
                                  created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(&def.id.0)
        .bind(&def.org_id.0)
        .bind(&def.name)
        .bind(stream_type_to_str(def.stream_type))
        .bind(Json(&def.schema))
        .bind(def.retention.map(Json))
        .bind(system)
        .bind(def.created_at.0)
        .bind(def.updated_at.0)
        .execute(&self.pool)
        .await;
        if let Err(error) = insert {
            if let sqlx::Error::Database(database_error) = &error
                && database_error.code().as_deref() == Some("23505")
                && database_error.constraint() == Some("uniq_streams_org_name_type")
            {
                return Err(Error::conflict(format!(
                    "stream `{}` with type `{}` already exists",
                    def.name,
                    def.stream_type.as_str()
                )));
            }
            return Err(sqlx_err(error));
        }
        Ok(def)
    }

    async fn update_schema(&self, id: &Id, schema: Schema) -> Result<()> {
        let now = TimestampMicros::now().0;
        // 先读出 (org, name, stream_type) 供 cache invalidate（在 UPDATE 之前以拿当前 schema）
        let row =
            sqlx::query("SELECT org_id, name, stream_type, system FROM streams WHERE id = $1")
                .bind(&id.0)
                .fetch_optional(&self.pool)
                .await
                .map_err(sqlx_err)?;

        if row
            .as_ref()
            .and_then(|value| value.try_get::<bool, _>("system").ok())
            .unwrap_or(false)
        {
            return Err(Error::forbidden("system stream schema is immutable"));
        }

        sqlx::query("UPDATE streams SET schema = $2, updated_at_micros = $3 WHERE id = $1")
            .bind(&id.0)
            .bind(Json(&schema))
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        if let Some(r) = row {
            let org_id: String = r.try_get("org_id").unwrap_or_default();
            let name: String = r.try_get("name").unwrap_or_default();
            let st: String = r.try_get("stream_type").unwrap_or_default();
            invalidate_stream_cache(&self.cache, &org_id, &name, &st);
        }
        Ok(())
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "streams")
    )]
    async fn update_schema_internal(&self, id: &Id, schema: Schema) -> Result<()> {
        let now = TimestampMicros::now().0;
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let row =
            sqlx::query("SELECT org_id, name, stream_type FROM streams WHERE id = $1 FOR UPDATE")
                .bind(&id.0)
                .fetch_optional(&mut *tx)
                .await
                .map_err(sqlx_err)?
                .ok_or_else(|| Error::not_found(format!("stream {}", id.0)))?;
        sqlx::query("SELECT set_config('molesignal.internal_system_mutation', 'true', true)")
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        sqlx::query("UPDATE streams SET schema = $2, updated_at_micros = $3 WHERE id = $1")
            .bind(&id.0)
            .bind(Json(&schema))
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        tx.commit().await.map_err(sqlx_err)?;

        let org_id: String = row.try_get("org_id").unwrap_or_default();
        let name: String = row.try_get("name").unwrap_or_default();
        let stream_type: String = row.try_get("stream_type").unwrap_or_default();
        invalidate_stream_cache(&self.cache, &org_id, &name, &stream_type);
        Ok(())
    }

    async fn update_retention(&self, id: &Id, retention: Option<Retention>) -> Result<()> {
        let now = TimestampMicros::now().0;
        let row = sqlx::query("SELECT org_id, name, stream_type FROM streams WHERE id = $1")
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::not_found(format!("stream {}", id.0)))?;

        sqlx::query("UPDATE streams SET retention = $2, updated_at_micros = $3 WHERE id = $1")
            .bind(&id.0)
            .bind(retention.map(Json))
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        let org_id: String = row.try_get("org_id").unwrap_or_default();
        let name: String = row.try_get("name").unwrap_or_default();
        let st: String = row.try_get("stream_type").unwrap_or_default();
        invalidate_stream_cache(&self.cache, &org_id, &name, &st);
        Ok(())
    }

    async fn get(
        &self,
        org_id: &Id,
        name: &str,
        stream_type: StreamType,
    ) -> Result<StreamDefinition> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM streams WHERE org_id = $1 AND name = $2 AND stream_type = $3"
        ))
        .bind(&org_id.0)
        .bind(name)
        .bind(stream_type_to_str(stream_type))
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row_to_stream(row)
    }

    async fn get_by_id(&self, id: &Id) -> Result<StreamDefinition> {
        let row = sqlx::query(&format!("SELECT {COLS} FROM streams WHERE id = $1"))
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        row_to_stream(row)
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<StreamDefinition>> {
        let rows = sqlx::query(&format!("SELECT {COLS} FROM streams WHERE org_id = $1"))
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_stream).collect()
    }

    async fn get_settings(&self, id: &Id) -> Result<StreamSettings> {
        let row = sqlx::query("SELECT settings FROM streams WHERE id = $1")
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::not_found(format!("stream {}", id.0)))?;
        let settings: Option<Json<StreamSettings>> = row.try_get("settings").map_err(sqlx_err)?;
        Ok(settings.map(|value| value.0).unwrap_or_default())
    }

    async fn update_settings(&self, id: &Id, settings: StreamSettings) -> Result<StreamSettings> {
        let now = TimestampMicros::now().0;
        let row = sqlx::query("SELECT org_id, name, stream_type FROM streams WHERE id = $1")
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::not_found(format!("stream {}", id.0)))?;

        sqlx::query("UPDATE streams SET settings = $2, updated_at_micros = $3 WHERE id = $1")
            .bind(&id.0)
            .bind(Json(&settings))
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        let org_id: String = row.try_get("org_id").unwrap_or_default();
        let name: String = row.try_get("name").unwrap_or_default();
        let st: String = row.try_get("stream_type").unwrap_or_default();
        invalidate_stream_cache(&self.cache, &org_id, &name, &st);
        Ok(settings)
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let row =
            sqlx::query("SELECT org_id, name, stream_type, system FROM streams WHERE id = $1")
                .bind(&id.0)
                .fetch_optional(&self.pool)
                .await
                .map_err(sqlx_err)?;

        if row
            .as_ref()
            .and_then(|value| value.try_get::<bool, _>("system").ok())
            .unwrap_or(false)
        {
            return Err(Error::forbidden("system stream cannot be deleted"));
        }

        sqlx::query("DELETE FROM streams WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        if let Some(r) = row {
            let org_id: String = r.try_get("org_id").unwrap_or_default();
            let name: String = r.try_get("name").unwrap_or_default();
            let st: String = r.try_get("stream_type").unwrap_or_default();
            invalidate_stream_cache(&self.cache, &org_id, &name, &st);
        }
        Ok(())
    }
}
