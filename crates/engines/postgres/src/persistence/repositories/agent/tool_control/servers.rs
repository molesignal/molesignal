// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MCP 服务器的读写。
//!
//! 密文放在 `agent_mcp_server_secrets`，一台服务器可以有几格：`credential` 是认证凭据，
//! `header:<名称>` 与 `env:<名称>` 是敏感的请求头与环境变量。服务器行里的 `headers`、`env`
//! 只记名称、是否敏感、明文值或末四位。

use std::collections::{HashMap, HashSet};

use sqlx::{PgConnection, Row, types::Json};

use super::{
    super::super::sqlx_err,
    PgToolControlRepository,
    rows::{SERVER_COLS, named_values, server_row},
};
use crate::{
    agent::{
        mcp::{NamedValue, NamedValueInput, last4},
        tool_control::{McpServer, McpServerInput, McpServerRuntime},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const CREDENTIAL_SLOT: &str = "credential";

/// 保存时要做的事：表里存的样子、要加密写入的格、要留下的旧格。
struct StoredValues {
    values: Vec<NamedValue>,
    sealed: Vec<(String, String)>,
    kept: Vec<String>,
}

/// 写入的请求头或环境变量 → 表里的样子。敏感值没填时沿用 `previous` 里同名的旧值；原来是
/// 明文、这次标成敏感又没填的，把原来的明文加密存下。
fn stored_values(prefix: &str, input: &[NamedValueInput], previous: &[NamedValue]) -> StoredValues {
    let previous: HashMap<&str, &NamedValue> = previous
        .iter()
        .map(|value| (value.name.as_str(), value))
        .collect();

    let mut stored = StoredValues {
        values: Vec::with_capacity(input.len()),
        sealed: Vec::new(),
        kept: Vec::new(),
    };

    for item in input {
        let slot = format!("{prefix}:{}", item.name);
        let old = previous.get(item.name.as_str());

        let value = match (item.secret, item.value.as_deref(), old) {
            (false, value, _) => NamedValue {
                name: item.name.clone(),
                secret: false,
                value: value.map(str::to_string),
                last4: None,
                set: value.is_some(),
            },
            (true, Some(value), _) => {
                stored.sealed.push((slot, value.to_string()));

                NamedValue {
                    name: item.name.clone(),
                    secret: true,
                    value: None,
                    last4: Some(last4(value)),
                    set: true,
                }
            }
            (true, None, Some(old)) if old.secret && old.set => {
                stored.kept.push(slot);

                NamedValue {
                    name: item.name.clone(),
                    secret: true,
                    value: None,
                    last4: old.last4.clone(),
                    set: true,
                }
            }
            (true, None, Some(old)) if !old.secret && old.value.is_some() => {
                let value = old.value.clone().unwrap_or_default();
                stored.sealed.push((slot, value.clone()));

                NamedValue {
                    name: item.name.clone(),
                    secret: true,
                    value: None,
                    last4: Some(last4(&value)),
                    set: true,
                }
            }
            (true, None, _) => NamedValue {
                name: item.name.clone(),
                secret: true,
                value: None,
                last4: None,
                set: false,
            },
        };

        stored.values.push(value);
    }

    stored
}

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
    async fn seal_slots(
        &self,
        connection: &mut PgConnection,
        org_id: &Id,
        server_id: &Id,
        slots: &[(String, String)],
    ) -> Result<()> {
        let now = TimestampMicros::now();

        for (slot, plaintext) in slots {
            let (nonce, ciphertext) = self.seal(plaintext)?;

            sqlx::query(
                "INSERT INTO agent_mcp_server_secrets
                    (server_id,slot,org_id,ciphertext,nonce,created_at_micros,updated_at_micros)
                 VALUES ($1,$2,$3,$4,$5,$6,$6)
                 ON CONFLICT (server_id,slot) DO UPDATE SET
                    org_id=EXCLUDED.org_id,ciphertext=EXCLUDED.ciphertext,nonce=EXCLUDED.nonce,
                    updated_at_micros=EXCLUDED.updated_at_micros",
            )
            .bind(&server_id.0)
            .bind(slot)
            .bind(&org_id.0)
            .bind(ciphertext)
            .bind(nonce)
            .bind(now.0)
            .execute(&mut *connection)
            .await
            .map_err(sqlx_err)?;
        }

        Ok(())
    }

    /// 写好请求头与环境变量的密文，删掉不再用的格。
    async fn write_values(
        &self,
        connection: &mut PgConnection,
        input: &McpServerInput,
        headers: &StoredValues,
        env: &StoredValues,
    ) -> Result<()> {
        let sealed = headers
            .sealed
            .iter()
            .chain(&env.sealed)
            .cloned()
            .collect::<Vec<_>>();

        self.seal_slots(&mut *connection, &input.org_id, &input.id, &sealed)
            .await?;

        let live = sealed
            .iter()
            .map(|(slot, _)| slot.clone())
            .chain(headers.kept.iter().cloned())
            .chain(env.kept.iter().cloned())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        sqlx::query(
            "DELETE FROM agent_mcp_server_secrets
             WHERE server_id=$1 AND (slot LIKE 'header:%' OR slot LIKE 'env:%')
               AND NOT (slot = ANY($2))",
        )
        .bind(&input.id.0)
        .bind(&live)
        .execute(&mut *connection)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    pub(super) async fn servers_in(&self, org_id: &Id) -> Result<Vec<McpServer>> {
        let rows = sqlx::query(&format!(
            "SELECT {SERVER_COLS} FROM agent_mcp_servers
             WHERE org_id=$1 ORDER BY updated_at_micros DESC,name"
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

        let rows = sqlx::query(
            "SELECT slot,ciphertext,nonce FROM agent_mcp_server_secrets
             WHERE org_id=$1 AND server_id=$2",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let mut secrets = HashMap::new();

        for row in rows {
            let slot: String = row.try_get("slot").map_err(sqlx_err)?;
            let ciphertext: Vec<u8> = row.try_get("ciphertext").map_err(sqlx_err)?;
            let nonce: Vec<u8> = row.try_get("nonce").map_err(sqlx_err)?;

            secrets.insert(slot, self.open(&nonce, &ciphertext)?);
        }

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
                 created_by,created_at_micros,updated_at_micros)
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
        .bind(now.0)
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
                max_response_bytes=$22,enabled=$23,updated_at_micros=$24,
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
        .bind(now.0)
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
                last_tested_at_micros=COALESCE($5,last_tested_at_micros),
                last_synced_at_micros=COALESCE($6,last_synced_at_micros),
                updated_at_micros=$7
             WHERE org_id=$1 AND id=$2",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(status)
        .bind(last_error)
        .bind(tested_at.map(|value| value.0))
        .bind(synced_at.map(|value| value.0))
        .bind(TimestampMicros::now().0)
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
            "UPDATE agent_mcp_servers SET enabled=$3,updated_at_micros=$4
             WHERE org_id=$1 AND id=$2",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(enabled)
        .bind(TimestampMicros::now().0)
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

        for table in [
            "agent_mcp_tools",
            "agent_mcp_prompts",
            "agent_mcp_server_secrets",
            "agent_mcp_member_credentials",
        ] {
            sqlx::query(&format!(
                "DELETE FROM {table} WHERE org_id=$1 AND server_id=$2"
            ))
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;
        }

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
               AND last_synced_at_micros IS NOT NULL
               AND COALESCE(last_tested_at_micros,last_synced_at_micros) < $1
             ORDER BY COALESCE(last_tested_at_micros,last_synced_at_micros)
             LIMIT $2"
        ))
        .bind(before.0)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(server_row).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(name: &str, secret: bool, value: Option<&str>) -> NamedValueInput {
        NamedValueInput {
            name: name.into(),
            secret,
            value: value.map(str::to_string),
        }
    }

    #[test]
    fn secrets_are_kept_when_left_blank_and_plain_values_move_into_a_slot() {
        let previous = vec![
            NamedValue {
                name: "authorization".into(),
                secret: true,
                value: None,
                last4: Some("abcd".into()),
                set: true,
            },
            NamedValue {
                name: "x-team".into(),
                secret: false,
                value: Some("platform".into()),
                last4: None,
                set: true,
            },
        ];

        let stored = stored_values(
            "header",
            &[
                input("authorization", true, None),
                input("x-team", true, None),
                input("x-new-key", true, None),
                input("x-org", false, Some("acme")),
            ],
            &previous,
        );

        assert_eq!(stored.kept, vec!["header:authorization".to_string()]);
        assert_eq!(stored.values[0].last4.as_deref(), Some("abcd"));
        assert_eq!(
            stored.sealed,
            vec![("header:x-team".to_string(), "platform".to_string())]
        );
        assert!(stored.values[1].secret && stored.values[1].set);
        assert!(!stored.values[2].set);
        assert_eq!(stored.values[3].value.as_deref(), Some("acme"));
    }
}
