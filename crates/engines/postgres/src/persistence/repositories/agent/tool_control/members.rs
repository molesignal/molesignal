// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 成员凭据与服务器的 prompt。
//!
//! 成员凭据按（服务器，成员）一行，只有本人的调用会解密它；列表里只有末四位。

use sqlx::{Row, types::Json};

use super::{
    super::super::sqlx_err,
    PgToolControlRepository,
    rows::{PROMPT_COLS, member_credential_row, prompt_row},
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
            "SELECT ciphertext,nonce FROM agent_mcp_member_credentials
             WHERE org_id=$1 AND server_id=$2 AND user_id=$3",
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
            "SELECT server_id,user_id,last4,updated_at_micros FROM agent_mcp_member_credentials
             WHERE org_id=$1 AND user_id=$2",
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
            "INSERT INTO agent_mcp_member_credentials
                (server_id,user_id,org_id,ciphertext,nonce,last4,created_at_micros,updated_at_micros)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$7)
             ON CONFLICT (server_id,user_id) DO UPDATE SET
                ciphertext=EXCLUDED.ciphertext,nonce=EXCLUDED.nonce,last4=EXCLUDED.last4,
                updated_at_micros=EXCLUDED.updated_at_micros",
        )
        .bind(&server_id.0)
        .bind(&user_id.0)
        .bind(&org_id.0)
        .bind(ciphertext)
        .bind(nonce)
        .bind(last4(value))
        .bind(now.0)
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
            "DELETE FROM agent_mcp_member_credentials
             WHERE org_id=$1 AND server_id=$2 AND user_id=$3",
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
        let rows = sqlx::query(&format!(
            "SELECT {PROMPT_COLS} FROM agent_mcp_prompts WHERE org_id=$1 ORDER BY server_id,name"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(prompt_row).collect()
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "agent_mcp_prompts")
    )]
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

        let mut transaction = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        sqlx::query("DELETE FROM agent_mcp_prompts WHERE org_id=$1 AND server_id=$2")
            .bind(&org_id.0)
            .bind(&server_id.0)
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;

        for prompt in &prompts {
            sqlx::query(
                "INSERT INTO agent_mcp_prompts
                    (server_id,name,org_id,title,description,arguments,synced_at_micros)
                 VALUES ($1,$2,$3,$4,$5,$6,$7)
                 ON CONFLICT (server_id,name) DO NOTHING",
            )
            .bind(&server_id.0)
            .bind(&prompt.name)
            .bind(&org_id.0)
            .bind(&prompt.title)
            .bind(&prompt.description)
            .bind(Json(&prompt.arguments))
            .bind(prompt.synced_at.0)
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;
        }

        transaction.commit().await.map_err(sqlx_err)?;
        Ok(())
    }
}
