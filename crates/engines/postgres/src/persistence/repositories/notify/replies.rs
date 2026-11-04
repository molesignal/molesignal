// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `notify_reply_refs`（回复短信、电话按键处理告警的引用）的 Pg 实装。

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::{
    domain::notify::reply::{NotifyReplyRepository, REPLY_SLOTS, ReplyRef},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgNotifyReplyRepository {
    pool: PgPool,
}

impl PgNotifyReplyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, organization_id, connector_id, line_key, slot, recipient_user_id,
    alert_ids, created_at, expires_at";

fn row_to(row: sqlx::postgres::PgRow) -> Result<ReplyRef> {
    let alert_ids: Json<Vec<String>> = row.try_get("alert_ids").map_err(sqlx_err)?;
    let slot: i16 = row.try_get("slot").map_err(sqlx_err)?;

    Ok(ReplyRef {
        id: row.try_get("id").map_err(sqlx_err)?,
        organization_id: Id::from_string(
            row.try_get::<String, _>("organization_id")
                .map_err(sqlx_err)?,
        ),
        connector_id: Id::from_string(row.try_get::<String, _>("connector_id").map_err(sqlx_err)?),
        line_key: row.try_get("line_key").map_err(sqlx_err)?,
        slot: u8::try_from(slot).unwrap_or_default(),
        recipient_user_id: row
            .try_get::<Option<String>, _>("recipient_user_id")
            .map_err(sqlx_err)?
            .map(Id::from_string),
        alert_ids: alert_ids.0.into_iter().map(Id::from_string).collect(),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        expires_at: row
            .try_get::<TimestampMicros, _>("expires_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl NotifyReplyRepository for PgNotifyReplyRepository {
    async fn record(&self, reply: ReplyRef) -> Result<ReplyRef> {
        let alert_ids: Vec<&str> = reply.alert_ids.iter().map(Id::as_str).collect();

        sqlx::query(
            "INSERT INTO notify_reply_refs
             (id, organization_id, connector_id, line_key, slot, recipient_user_id,
              alert_ids, created_at, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(&reply.id)
        .bind(&reply.organization_id.0)
        .bind(&reply.connector_id.0)
        .bind(&reply.line_key)
        .bind(i16::from(reply.slot))
        .bind(reply.recipient_user_id.as_ref().map(|id| &id.0))
        .bind(Json(&alert_ids))
        .bind(reply.created_at)
        .bind(reply.expires_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(reply)
    }

    async fn get(&self, id: &str, now: TimestampMicros) -> Result<Option<ReplyRef>> {
        sqlx::query(&format!(
            "SELECT {COLS} FROM notify_reply_refs WHERE id = $1 AND expires_at > $2"
        ))
        .bind(id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(row_to)
        .transpose()
    }

    async fn find(
        &self,
        organization_id: &Id,
        connector_id: &Id,
        line_key: &str,
        slot: Option<u8>,
        now: TimestampMicros,
    ) -> Result<Option<ReplyRef>> {
        sqlx::query(&format!(
            "SELECT {COLS} FROM notify_reply_refs
              WHERE organization_id = $1 AND connector_id = $2 AND line_key = $3
                AND expires_at > $4
                AND ($5::SMALLINT IS NULL OR slot = $5)
           ORDER BY created_at DESC
              LIMIT 1"
        ))
        .bind(&organization_id.0)
        .bind(&connector_id.0)
        .bind(line_key)
        .bind(now)
        .bind(slot.map(i16::from))
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(row_to)
        .transpose()
    }

    async fn bind_call(&self, id: &str, call_id: &str) -> Result<()> {
        sqlx::query("UPDATE notify_reply_refs SET call_id = $2 WHERE id = $1")
            .bind(id)
            .bind(call_id)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }

    async fn find_by_call(
        &self,
        connector_id: &Id,
        call_id: &str,
        now: TimestampMicros,
    ) -> Result<Option<ReplyRef>> {
        sqlx::query(&format!(
            "SELECT {COLS} FROM notify_reply_refs
              WHERE connector_id = $1 AND call_id = $2 AND expires_at > $3
           ORDER BY created_at DESC
              LIMIT 1"
        ))
        .bind(&connector_id.0)
        .bind(call_id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(row_to)
        .transpose()
    }

    async fn next_slot(
        &self,
        organization_id: &Id,
        connector_id: &Id,
        line_key: &str,
    ) -> Result<u8> {
        let last: Option<i16> = sqlx::query_scalar(
            "SELECT slot FROM notify_reply_refs
              WHERE organization_id = $1 AND connector_id = $2 AND line_key = $3
           ORDER BY created_at DESC
              LIMIT 1",
        )
        .bind(&organization_id.0)
        .bind(&connector_id.0)
        .bind(line_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(match last {
            Some(slot) => (u8::try_from(slot).unwrap_or_default() + 1) % REPLY_SLOTS,
            None => 1,
        })
    }
}
