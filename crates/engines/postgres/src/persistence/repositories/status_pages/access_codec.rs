// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use sqlx::Row;

use super::PgStatusPageRepository;
use crate::{
    domain::status_page::{
        StatusPageAccessRule, StatusPageAccessRuleKind, StatusPageAccessSession,
        StatusPageMagicLink,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

impl PgStatusPageRepository {
    pub(super) fn row_to_access_rule(
        &self,
        row: sqlx::postgres::PgRow,
    ) -> Result<StatusPageAccessRule> {
        let kind: String = row.try_get("kind").map_err(super::sqlx_err)?;
        let ciphertext: Vec<u8> = row.try_get("value_ciphertext").map_err(super::sqlx_err)?;
        let nonce: Vec<u8> = row.try_get("value_nonce").map_err(super::sqlx_err)?;
        Ok(StatusPageAccessRule {
            id: Id(row.try_get("id").map_err(super::sqlx_err)?),
            org_id: Id(row.try_get("org_id").map_err(super::sqlx_err)?),
            status_page_id: Id(row.try_get("status_page_id").map_err(super::sqlx_err)?),
            kind: StatusPageAccessRuleKind::parse(&kind)
                .ok_or_else(|| Error::internal(format!("unknown access rule kind: {kind}")))?,
            value: self.open_access_value(&nonce, &ciphertext)?,
            created_at: TimestampMicros(row.try_get("created_at_micros").map_err(super::sqlx_err)?),
            updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(super::sqlx_err)?),
        })
    }

    pub(super) fn row_to_magic_link(
        &self,
        row: &sqlx::postgres::PgRow,
    ) -> Result<StatusPageMagicLink> {
        let ciphertext: Vec<u8> = row.try_get("email_ciphertext").map_err(super::sqlx_err)?;
        let nonce: Vec<u8> = row.try_get("email_nonce").map_err(super::sqlx_err)?;
        Ok(StatusPageMagicLink {
            id: Id(row.try_get("id").map_err(super::sqlx_err)?),
            org_id: Id(row.try_get("org_id").map_err(super::sqlx_err)?),
            status_page_id: Id(row.try_get("status_page_id").map_err(super::sqlx_err)?),
            access_rule_id: Id(row.try_get("access_rule_id").map_err(super::sqlx_err)?),
            email: self.open_access_value(&nonce, &ciphertext)?,
            token_hash: row.try_get("token_hash").map_err(super::sqlx_err)?,
            origin_host: row.try_get("origin_host").map_err(super::sqlx_err)?,
            expires_at: TimestampMicros(row.try_get("expires_at_micros").map_err(super::sqlx_err)?),
            consumed_at: row
                .try_get::<Option<i64>, _>("consumed_at_micros")
                .map_err(super::sqlx_err)?
                .map(TimestampMicros),
            created_at: TimestampMicros(row.try_get("created_at_micros").map_err(super::sqlx_err)?),
        })
    }

    pub(super) fn row_to_access_session(
        &self,
        row: sqlx::postgres::PgRow,
    ) -> Result<StatusPageAccessSession> {
        let ciphertext: Vec<u8> = row.try_get("email_ciphertext").map_err(super::sqlx_err)?;
        let nonce: Vec<u8> = row.try_get("email_nonce").map_err(super::sqlx_err)?;
        Ok(StatusPageAccessSession {
            id: Id(row.try_get("id").map_err(super::sqlx_err)?),
            org_id: Id(row.try_get("org_id").map_err(super::sqlx_err)?),
            status_page_id: Id(row.try_get("status_page_id").map_err(super::sqlx_err)?),
            access_rule_id: Id(row.try_get("access_rule_id").map_err(super::sqlx_err)?),
            email: self.open_access_value(&nonce, &ciphertext)?,
            session_token_hash: row.try_get("session_token_hash").map_err(super::sqlx_err)?,
            origin_host: row.try_get("origin_host").map_err(super::sqlx_err)?,
            expires_at: TimestampMicros(row.try_get("expires_at_micros").map_err(super::sqlx_err)?),
            revoked_at: row
                .try_get::<Option<i64>, _>("revoked_at_micros")
                .map_err(super::sqlx_err)?
                .map(TimestampMicros),
            last_seen_at: TimestampMicros(
                row.try_get("last_seen_at_micros")
                    .map_err(super::sqlx_err)?,
            ),
            created_at: TimestampMicros(row.try_get("created_at_micros").map_err(super::sqlx_err)?),
        })
    }
}
