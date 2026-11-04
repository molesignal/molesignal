// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MCP 服务器的密文，存在 `sealed_secrets` 里 `owner_kind = 'mcp_server'` 的行。一台服务器可以有几格：
//! `credential` 是认证凭据，`header:<名称>` 与 `env:<名称>` 是敏感的请求头与环境变量。服务器行里的
//! `headers`、`env` 只记名称、是否敏感、明文值或末四位。成员凭据（`mcp_member`）在 [`super::members`]。

use std::collections::{HashMap, HashSet};

use sqlx::{PgConnection, Row};

use super::{super::super::sqlx_err, PgToolControlRepository};
use crate::{
    agent::{
        mcp::{NamedValue, NamedValueInput, last4},
        tool_control::McpServerInput,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub(super) const CREDENTIAL_SLOT: &str = "credential";

/// 保存时要做的事：表里存的样子、要加密写入的格、要留下的旧格。
pub(super) struct StoredValues {
    pub(super) values: Vec<NamedValue>,
    sealed: Vec<(String, String)>,
    kept: Vec<String>,
}

/// 写入的请求头或环境变量 → 表里的样子。敏感值没填时沿用 `previous` 里同名的旧值；原来是
/// 明文、这次标成敏感又没填的，把原来的明文加密存下。
pub(super) fn stored_values(
    prefix: &str,
    input: &[NamedValueInput],
    previous: &[NamedValue],
) -> StoredValues {
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

/// 删掉一台服务器的密文和成员凭据。
pub(super) async fn remove_server_secrets(
    connection: &mut PgConnection,
    org_id: &Id,
    server_id: &Id,
) -> Result<()> {
    sqlx::query(
        "DELETE FROM sealed_secrets
         WHERE org_id=$1 AND owner_kind IN ('mcp_server','mcp_member') AND owner_id=$2",
    )
    .bind(&org_id.0)
    .bind(&server_id.0)
    .execute(&mut *connection)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

impl PgToolControlRepository {
    pub(super) async fn seal_slots(
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
                "INSERT INTO sealed_secrets
                    (org_id,owner_kind,owner_id,slot,ciphertext,nonce,created_at,updated_at)
                 VALUES ($1,'mcp_server',$2,$3,$4,$5,$6,$6)
                 ON CONFLICT (org_id,owner_kind,owner_id,slot) DO UPDATE SET
                    ciphertext=EXCLUDED.ciphertext,nonce=EXCLUDED.nonce,
                    updated_at=EXCLUDED.updated_at",
            )
            .bind(&org_id.0)
            .bind(&server_id.0)
            .bind(slot)
            .bind(ciphertext)
            .bind(nonce)
            .bind(now)
            .execute(&mut *connection)
            .await
            .map_err(sqlx_err)?;
        }

        Ok(())
    }

    /// 写好请求头与环境变量的密文，删掉不再用的格。
    pub(super) async fn write_values(
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
            "DELETE FROM sealed_secrets
             WHERE org_id=$1 AND owner_kind='mcp_server' AND owner_id=$2
               AND (slot LIKE 'header:%' OR slot LIKE 'env:%')
               AND NOT (slot = ANY($3))",
        )
        .bind(&input.org_id.0)
        .bind(&input.id.0)
        .bind(&live)
        .execute(&mut *connection)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    /// 解开一台服务器的所有格，按格名取。
    pub(super) async fn server_secrets(
        &self,
        org_id: &Id,
        server_id: &Id,
    ) -> Result<HashMap<String, String>> {
        let rows = sqlx::query(
            "SELECT slot,ciphertext,nonce FROM sealed_secrets
             WHERE org_id=$1 AND owner_kind='mcp_server' AND owner_id=$2",
        )
        .bind(&org_id.0)
        .bind(&server_id.0)
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

        Ok(secrets)
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
