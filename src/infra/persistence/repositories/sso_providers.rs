// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `sso_providers` Postgres 实装。provider kind 映射字符串，config 落 JSONB。

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::iam::{SsoProvider, SsoProviderConfig, SsoProviderKind, SsoProviderRepository},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgSsoProviderRepository {
    pool: PgPool,
}

impl PgSsoProviderRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str =
    "id, org_id, name, provider, enabled, config, created_at_micros, updated_at_micros";

fn kind_to_str(kind: SsoProviderKind) -> &'static str {
    match kind {
        SsoProviderKind::Oidc => "oidc",
        SsoProviderKind::Saml => "saml",
        SsoProviderKind::Ldap => "ldap",
    }
}

fn kind_from_str(s: &str) -> Result<SsoProviderKind> {
    match s {
        "oidc" => Ok(SsoProviderKind::Oidc),
        "saml" => Ok(SsoProviderKind::Saml),
        "ldap" => Ok(SsoProviderKind::Ldap),
        other => Err(Error::internal(format!(
            "unknown sso_providers.provider value: {other}"
        ))),
    }
}

fn row_to_provider(row: sqlx::postgres::PgRow) -> Result<SsoProvider> {
    let kind_str: String = row.try_get("provider").map_err(sqlx_err)?;
    let kind = kind_from_str(&kind_str)?;
    let config_json: Json<serde_json::Value> = row.try_get("config").map_err(sqlx_err)?;
    let config_value = upgrade_config(kind, config_json.0);
    // 用 kind 引导 deserialize，让 Oidc/Saml variant 不被 untagged 误判。
    let config: SsoProviderConfig = match kind {
        SsoProviderKind::Oidc => SsoProviderConfig::Oidc(
            serde_json::from_value(config_value)
                .map_err(|e| Error::internal(format!("sso oidc config decode: {e}")))?,
        ),
        SsoProviderKind::Saml => SsoProviderConfig::Saml(
            serde_json::from_value(config_value)
                .map_err(|e| Error::internal(format!("sso saml config decode: {e}")))?,
        ),
        SsoProviderKind::Ldap => SsoProviderConfig::Ldap(
            serde_json::from_value(config_value)
                .map_err(|e| Error::internal(format!("sso ldap config decode: {e}")))?,
        ),
    };
    Ok(SsoProvider {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        kind,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        config,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

fn upgrade_config(kind: SsoProviderKind, mut value: serde_json::Value) -> serde_json::Value {
    if kind != SsoProviderKind::Ldap
        || value
            .get("field_mapping")
            .is_some_and(|mapping| mapping.is_object())
    {
        return value;
    }
    let Some(config) = value.as_object_mut() else {
        return value;
    };
    let legacy = |name: &str, fallback: &str| -> String {
        config
            .get(name)
            .and_then(serde_json::Value::as_str)
            .unwrap_or(fallback)
            .to_owned()
    };
    let email = legacy("email_attribute", "mail");
    let display_name = legacy("display_name_attribute", "displayName");
    let groups = legacy("group_attribute", "memberOf");
    config.insert(
        "field_mapping".into(),
        serde_json::json!({
            "subject": "dn",
            "email": email,
            "display_name": display_name,
            "groups": groups,
        }),
    );
    value
}

fn config_to_value(config: &SsoProviderConfig) -> serde_json::Value {
    match config {
        SsoProviderConfig::Oidc(c) => serde_json::to_value(c).expect("oidc config serializes"),
        SsoProviderConfig::Saml(c) => serde_json::to_value(c).expect("saml config serializes"),
        SsoProviderConfig::Ldap(c) => serde_json::to_value(c).expect("ldap config serializes"),
    }
}

#[async_trait]
impl SsoProviderRepository for PgSsoProviderRepository {
    async fn create(&self, p: SsoProvider) -> Result<SsoProvider> {
        sqlx::query(
            "INSERT INTO sso_providers
                (id, org_id, name, provider, enabled, config, created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&p.id.0)
        .bind(&p.org_id.0)
        .bind(&p.name)
        .bind(kind_to_str(p.kind))
        .bind(p.enabled)
        .bind(Json(config_to_value(&p.config)))
        .bind(p.created_at.0)
        .bind(p.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(p)
    }

    async fn update(&self, p: SsoProvider) -> Result<SsoProvider> {
        let res = sqlx::query(
            "UPDATE sso_providers
                SET name = $2,
                    provider = $3,
                    enabled = $4,
                    config = $5,
                    updated_at_micros = $6
              WHERE id = $1",
        )
        .bind(&p.id.0)
        .bind(&p.name)
        .bind(kind_to_str(p.kind))
        .bind(p.enabled)
        .bind(Json(config_to_value(&p.config)))
        .bind(p.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found(format!("sso_provider {}", p.id.0)));
        }
        Ok(p)
    }

    async fn get(&self, id: &Id) -> Result<SsoProvider> {
        let row = sqlx::query(&format!("SELECT {COLS} FROM sso_providers WHERE id = $1"))
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::not_found(format!("sso_provider {}", id.0)))?;
        row_to_provider(row)
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<SsoProvider>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM sso_providers WHERE org_id = $1 ORDER BY name"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_provider).collect()
    }

    async fn list_enabled(&self, org_id: &Id) -> Result<Vec<SsoProvider>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM sso_providers
              WHERE org_id = $1 AND enabled = TRUE
              ORDER BY name"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_provider).collect()
    }

    async fn list_enabled_by_kind(&self, kind: SsoProviderKind) -> Result<Vec<SsoProvider>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM sso_providers
              WHERE provider = $1 AND enabled = TRUE
              ORDER BY org_id, name"
        ))
        .bind(kind_to_str(kind))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_provider).collect()
    }

    async fn set_enabled(&self, id: &Id, enabled: bool) -> Result<SsoProvider> {
        let now = TimestampMicros::now().0;
        let res = sqlx::query(
            "UPDATE sso_providers SET enabled = $2, updated_at_micros = $3 WHERE id = $1",
        )
        .bind(&id.0)
        .bind(enabled)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found(format!("sso_provider {}", id.0)));
        }
        self.get(id).await
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let res = sqlx::query("DELETE FROM sso_providers WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found(format!("sso_provider {}", id.0)));
        }
        Ok(())
    }
}
