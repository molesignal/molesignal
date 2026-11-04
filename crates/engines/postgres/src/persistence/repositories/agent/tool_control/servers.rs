// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MCP 服务器的读写。密文在 [`super::server_secrets`]。

use sqlx::types::Json;

use super::{
    super::super::sqlx_err,
    PgToolControlRepository,
    rows::{SERVER_COLS, named_values, server_row},
    server_secrets::{CREDENTIAL_SLOT, remove_server_secrets, stored_values},
};
use crate::{
    agent::{
        mcp::last4,
        tool_control::{McpServer, McpServerInput, McpServerRuntime},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 名称或前缀撞上唯一约束时说清楚是哪一个。
fn server_conflict(error: sqlx::Error, name: &str) -> Error {
    match &error {
        sqlx::Error::Database(database) if database.code().as_deref() == Some("23505") => {
            if database.constraint() == Some("uq_agent_mcp_servers_org_handle") {
                Error::conflict("another MCP server was created at the same time; try again")
            } else {
                Error::conflict(format!(
                    "an MCP server named `{name}` already exists in this scope"
                ))
            }
        }
        _ => sqlx_err(error),
    }
}

impl PgToolControlRepository {
    pub(super) async fn servers_in(&self, org_id: &Id) -> Result<Vec<McpServer>> {
        let rows = sqlx::query(&format!(
            "SELECT {SERVER_COLS} FROM agent_mcp_servers
             WHERE org_id=$1 ORDER BY updated_at DESC,name"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(server_row).collect()
    }

    pub(super) async fn server(&self, org_id: &Id, id: &Id) -> Result<McpServer> {
        let row = sqlx::query(&format!(
            "SELECT {SERVER_COLS} FROM agent_mcp_servers WHERE org_id=$1 AND id=$2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("MCP Server `{}` not found", id.0)))?;

        server_row(row)
    }

    pub(super) async fn server_runtime(
        &self,
        org_id: &Id,
        id: &Id,
        member: Option<&Id>,
    ) -> Result<McpServerRuntime> {
        let server = self.server(org_id, id).await?;

        let mut secrets = self.server_secrets(org_id, id).await?;

        let headers = server
            .headers
            .iter()
            .filter_map(|header| {
                let value = if header.secret {
                    secrets.get(&format!("header:{}", header.name)).cloned()
                } else {
                    header.value.clone()
                };

                value.map(|value| (header.name.clone(), value))
            })
            .collect();

        let env = server
            .env
            .iter()
            .filter_map(|variable| {
                let value = if variable.secret {
                    secrets.get(&format!("env:{}", variable.name)).cloned()
                } else {
                    variable.value.clone()
                };

                value.map(|value| (variable.name.clone(), value))
            })
            .collect();

        let member_credential = match member {
            Some(user_id) => self.member_secret(org_id, id, user_id).await?,
            None => None,
        };

        Ok(McpServerRuntime {
            credential: secrets.remove(CREDENTIAL_SLOT),
            headers,
            env,
            member_credential,
            server,
        })
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "agent_mcp_servers")
    )]
    pub(super) async fn insert_server(
        &self,
        input: McpServerInput,
        credential: Option<&str>,
    ) -> Result<McpServer> {
        let now = TimestampMicros::now();
        let headers = stored_values("header", &input.headers, &[]);
        let env = stored_values("env", &input.env, &[]);
        let mut transaction = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        sqlx::query(
            "INSERT INTO agent_mcp_servers
                (id,org_id,scope,team_id,owner_id,scope_key,handle,name,transport,endpoint_url,
                 command_template,auth_type,auth_header,credential_last4,credential_set,
                 member_credential,headers,env,private_only,allowed_domains,allowed_cidrs,
                 follow_redirects,tls_verify,timeout_ms,max_response_bytes,enabled,status,
                 created_by,created_at,updated_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,
                     $21,$22,$23,$24,$25,$26,'unavailable',$27,$28,$28)",
        )
        .bind(&input.id.0)
        .bind(&input.org_id.0)
        .bind(input.scope.kind())
        .bind(input.scope.team_id().map(|id| id.0.as_str()))
        .bind(input.scope.owner_id().map(|id| id.0.as_str()))
        .bind(input.scope.key())
        .bind(&input.handle)
        .bind(&input.name)
        .bind(&input.transport)
        .bind(&input.endpoint_url)
        .bind(&input.command_template)
        .bind(&input.auth_type)
        .bind(&input.auth_header)
        .bind(credential.map(last4))
        .bind(credential.is_some())
        .bind(input.member_credential.as_ref().map(Json))
        .bind(Json(&headers.values))
        .bind(Json(&env.values))
        .bind(input.private_only)
        .bind(Json(&input.allowed_domains))
        .bind(Json(&input.allowed_cidrs))
        .bind(input.follow_redirects)
        .bind(input.tls_verify)
        .bind(input.timeout_ms)
        .bind(input.max_response_bytes)
        .bind(input.enabled)
        .bind(&input.created_by.0)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(|error| server_conflict(error, &input.name))?;

        if let Some(credential) = credential {
            let slots = [(CREDENTIAL_SLOT.to_string(), credential.to_string())];
            self.seal_slots(&mut transaction, &input.org_id, &input.id, &slots)
                .await?;
        }

        self.write_values(&mut transaction, &input, &headers, &env)
            .await?;

        transaction.commit().await.map_err(sqlx_err)?;
        self.server(&input.org_id, &input.id).await
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "agent_mcp_servers")
    )]
    pub(super) async fn rewrite_server(
        &self,
        input: McpServerInput,
        credential: Option<&str>,
    ) -> Result<McpServer> {
        let now = TimestampMicros::now();
        let mut transaction = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let previous = sqlx::query(
            "SELECT headers,env FROM agent_mcp_servers WHERE org_id=$1 AND id=$2 FOR UPDATE",
        )
        .bind(&input.org_id.0)
        .bind(&input.id.0)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("MCP Server `{}` not found", input.id.0)))?;

        let headers = stored_values(
            "header",
            &input.headers,
            &named_values(&previous, "headers")?,
        );
        let env = stored_values("env", &input.env, &named_values(&previous, "env")?);

        sqlx::query(
            "UPDATE agent_mcp_servers SET
                scope=$3,team_id=$4,owner_id=$5,scope_key=$6,name=$7,transport=$8,
                endpoint_url=$9,command_template=$10,auth_type=$11,auth_header=$12,
                member_credential=$13,headers=$14,env=$15,private_only=$16,allowed_domains=$17,
                allowed_cidrs=$18,follow_redirects=$19,tls_verify=$20,timeout_ms=$21,
                max_response_bytes=$22,enabled=$23,updated_at=$24,
                credential_last4=COALESCE($25,credential_last4),
                credential_set=credential_set OR $26
             WHERE org_id=$1 AND id=$2",
        )
        .bind(&input.org_id.0)
        .bind(&input.id.0)
        .bind(input.scope.kind())
        .bind(input.scope.team_id().map(|id| id.0.as_str()))
        .bind(input.scope.owner_id().map(|id| id.0.as_str()))
        .bind(input.scope.key())
        .bind(&input.name)
        .bind(&input.transport)
        .bind(&input.endpoint_url)
        .bind(&input.command_template)
        .bind(&input.auth_type)
        .bind(&input.auth_header)
        .bind(input.member_credential.as_ref().map(Json))
        .bind(Json(&headers.values))
        .bind(Json(&env.values))
        .bind(input.private_only)
        .bind(Json(&input.allowed_domains))
        .bind(Json(&input.allowed_cidrs))
        .bind(input.follow_redirects)
        .bind(input.tls_verify)
        .bind(input.timeout_ms)
        .bind(input.max_response_bytes)
        .bind(input.enabled)
        .bind(now)
        .bind(credential.map(last4))
        .bind(credential.is_some())
        .execute(&mut *transaction)
        .await
        .map_err(|error| server_conflict(error, &input.name))?;

        if let Some(credential) = credential {
            let slots = [(CREDENTIAL_SLOT.to_string(), credential.to_string())];
            self.seal_slots(&mut transaction, &input.org_id, &input.id, &slots)
                .await?;
        }

        self.write_values(&mut transaction, &input, &headers, &env)
            .await?;

        transaction.commit().await.map_err(sqlx_err)?;
        self.server(&input.org_id, &input.id).await
    }

    pub(super) async fn set_status(
        &self,
        org_id: &Id,
        id: &Id,
        status: &str,
        last_error: Option<&str>,
        tested_at: Option<TimestampMicros>,
        synced_at: Option<TimestampMicros>,
    ) -> Result<McpServer> {
        let result = sqlx::query(
            "UPDATE agent_mcp_servers SET
                status=$3,last_error=$4,
                last_tested_at=COALESCE($5,last_tested_at),
                last_synced_at=COALESCE($6,last_synced_at),
                updated_at=$7
             WHERE org_id=$1 AND id=$2",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(status)
        .bind(last_error)
        .bind(tested_at)
        .bind(synced_at)
        .bind(TimestampMicros::now())
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if result.rows_affected() == 0 {
            return Err(Error::not_found(format!("MCP Server `{}` not found", id.0)));
        }

        self.server(org_id, id).await
    }

    pub(super) async fn set_enabled(
        &self,
        org_id: &Id,
        id: &Id,
        enabled: bool,
    ) -> Result<McpServer> {
        let result = sqlx::query(
            "UPDATE agent_mcp_servers SET enabled=$3,updated_at=$4
             WHERE org_id=$1 AND id=$2",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(enabled)
        .bind(TimestampMicros::now())
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if result.rows_affected() == 0 {
            return Err(Error::not_found(format!("MCP Server `{}` not found", id.0)));
        }

        self.server(org_id, id).await
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "agent_mcp_servers")
    )]
    pub(super) async fn remove_server(&self, org_id: &Id, id: &Id) -> Result<()> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        sqlx::query("DELETE FROM agent_mcp_tools WHERE org_id=$1 AND server_id=$2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;

        remove_server_secrets(&mut transaction, org_id, id).await?;

        let result = sqlx::query("DELETE FROM agent_mcp_servers WHERE org_id=$1 AND id=$2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;

        if result.rows_affected() == 0 {
            return Err(Error::not_found(format!("MCP Server `{}` not found", id.0)));
        }

        transaction.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    pub(super) async fn servers_due(
        &self,
        before: TimestampMicros,
        limit: i64,
    ) -> Result<Vec<McpServer>> {
        let rows = sqlx::query(&format!(
            "SELECT {SERVER_COLS} FROM agent_mcp_servers
             WHERE enabled AND transport IN ('streamable_http','sse')
               AND auth_type <> 'member_secret'
               AND last_synced_at IS NOT NULL
               AND COALESCE(last_tested_at,last_synced_at) < $1
             ORDER BY COALESCE(last_tested_at,last_synced_at)
             LIMIT $2"
        ))
        .bind(before)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(server_row).collect()
    }
}
