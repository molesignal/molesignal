// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 成员凭据与服务器的 prompt。
//!
//! 成员凭据存在 `sealed_secrets` 里 `owner_kind = 'mcp_member'` 的行，按（服务器，成员）一行：`owner_id`
//! 是服务器，`slot` 是成员。只有本人的调用会解密它；列表里只有末四位。

use sqlx::{Row, types::Json};

use super::{
    super::super::sqlx_err,
    PgToolControlRepository,
    rows::{StoredPrompt, member_credential_row, stored_prompts},
};
use crate::{
    agent::{
        mcp::{McpPrompt, last4},
        tool_control::MemberCredential,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

impl PgToolControlRepository {
    pub(super) async fn member_secret(
        &self,
        org_id: &Id,
        server_id: &Id,
        user_id: &Id,
    ) -> Result<Option<String>> {
        let row = sqlx::query(
            "SELECT ciphertext,nonce FROM sealed_secrets
             WHERE org_id=$1 AND owner_kind='mcp_member' AND owner_id=$2 AND slot=$3",
        )
        .bind(&org_id.0)
        .bind(&server_id.0)
        .bind(&user_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let Some(row) = row else {
            return Ok(None);
        };

        let ciphertext: Vec<u8> = row.try_get("ciphertext").map_err(sqlx_err)?;
        let nonce: Vec<u8> = row.try_get("nonce").map_err(sqlx_err)?;

        self.open(&nonce, &ciphertext).map(Some)
    }

    pub(super) async fn member_credentials(
        &self,
        org_id: &Id,
        user_id: &Id,
    ) -> Result<Vec<MemberCredential>> {
        let rows = sqlx::query(
            "SELECT owner_id AS server_id,slot AS user_id,last4,updated_at FROM sealed_secrets
             WHERE org_id=$1 AND owner_kind='mcp_member' AND slot=$2",
        )
        .bind(&org_id.0)
        .bind(&user_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(member_credential_row).collect()
    }

    pub(super) async fn save_member_credential(
        &self,
        org_id: &Id,
        server_id: &Id,
        user_id: &Id,
        value: &str,
    ) -> Result<MemberCredential> {
        let (nonce, ciphertext) = self.seal(value)?;
        let now = TimestampMicros::now();

        sqlx::query(
            "INSERT INTO sealed_secrets
                (org_id,owner_kind,owner_id,slot,ciphertext,nonce,last4,created_at,updated_at)
             VALUES ($1,'mcp_member',$2,$3,$4,$5,$6,$7,$7)
             ON CONFLICT (org_id,owner_kind,owner_id,slot) DO UPDATE SET
                ciphertext=EXCLUDED.ciphertext,nonce=EXCLUDED.nonce,last4=EXCLUDED.last4,
                updated_at=EXCLUDED.updated_at",
        )
        .bind(&org_id.0)
        .bind(&server_id.0)
        .bind(&user_id.0)
        .bind(ciphertext)
        .bind(nonce)
        .bind(last4(value))
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(MemberCredential {
            server_id: server_id.clone(),
            user_id: user_id.clone(),
            last4: last4(value),
            updated_at: now,
        })
    }

    pub(super) async fn remove_member_credential(
        &self,
        org_id: &Id,
        server_id: &Id,
        user_id: &Id,
    ) -> Result<bool> {
        let result = sqlx::query(
            "DELETE FROM sealed_secrets
             WHERE org_id=$1 AND owner_kind='mcp_member' AND owner_id=$2 AND slot=$3",
        )
        .bind(&org_id.0)
        .bind(&server_id.0)
        .bind(&user_id.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(result.rows_affected() > 0)
    }

    pub(super) async fn prompts_in(&self, org_id: &Id) -> Result<Vec<McpPrompt>> {
        let rows = sqlx::query(
            "SELECT id, prompts FROM agent_mcp_servers
             WHERE org_id=$1 AND prompts <> '[]'::JSONB
             ORDER BY id",
        )
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let mut prompts = Vec::new();

        for row in rows {
            let server_id = Id(row.try_get("id").map_err(sqlx_err)?);
            let stored: Json<Vec<StoredPrompt>> = row.try_get("prompts").map_err(sqlx_err)?;

            prompts.extend(
                stored
                    .0
                    .into_iter()
                    .map(|prompt| prompt.into_prompt(org_id, &server_id)),
            );
        }

        Ok(prompts)
    }

    pub(super) async fn put_prompts(
        &self,
        org_id: &Id,
        server_id: &Id,
        prompts: Vec<McpPrompt>,
    ) -> Result<()> {
        if prompts
            .iter()
            .any(|prompt| prompt.org_id != *org_id || prompt.server_id != *server_id)
        {
            return Err(Error::invalid(
                "MCP prompts must belong to the server they are saved for",
            ));
        }

        sqlx::query("UPDATE agent_mcp_servers SET prompts=$3 WHERE org_id=$1 AND id=$2")
            .bind(&org_id.0)
            .bind(&server_id.0)
            .bind(Json(stored_prompts(prompts)))
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }
}
