// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Email allowlist and one-time Magic Link access for private Status Pages.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusPageAccessRuleKind {
    Email,
    Domain,
}

impl StatusPageAccessRuleKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Domain => "domain",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "email" => Some(Self::Email),
            "domain" => Some(Self::Domain),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusPageAccessRule {
    pub id: Id,
    pub org_id: Id,
    pub status_page_id: Id,
    pub kind: StatusPageAccessRuleKind,
    pub value: String,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone)]
pub struct StatusPageMagicLink {
    pub id: Id,
    pub org_id: Id,
    pub status_page_id: Id,
    pub access_rule_id: Id,
    pub email: String,
    pub token_hash: String,
    pub origin_host: String,
    pub expires_at: TimestampMicros,
    pub consumed_at: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusPageAccessSession {
    pub id: Id,
    pub org_id: Id,
    pub status_page_id: Id,
    pub access_rule_id: Id,
    pub email: String,
    #[serde(skip_serializing)]
    pub session_token_hash: String,
    pub origin_host: String,
    pub expires_at: TimestampMicros,
    pub revoked_at: Option<TimestampMicros>,
    pub last_seen_at: TimestampMicros,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone)]
pub struct ConsumedStatusPageMagicLink {
    pub link: StatusPageMagicLink,
    pub session: StatusPageAccessSession,
}

#[derive(Debug, Clone)]
pub struct StatusPageMagicLinkExchange {
    pub page_id: Id,
    pub token_hash: String,
    pub origin_host: String,
    pub session_id: Id,
    pub session_token_hash: String,
    pub session_expires_at: TimestampMicros,
    pub now: TimestampMicros,
}

#[async_trait]
pub trait StatusPageAccessRepository: Send + Sync {
    async fn create_access_rule(&self, rule: StatusPageAccessRule) -> Result<StatusPageAccessRule>;
    async fn list_access_rules(
        &self,
        org_id: &Id,
        page_id: &Id,
    ) -> Result<Vec<StatusPageAccessRule>>;
    async fn find_access_rule_for_email(
        &self,
        org_id: &Id,
        page_id: &Id,
        email: &str,
        domain: &str,
    ) -> Result<Option<StatusPageAccessRule>>;
    async fn delete_access_rule(&self, org_id: &Id, page_id: &Id, rule_id: &Id) -> Result<()>;
    /// Returns `false` when a bounded anti-abuse cooldown suppresses delivery.
    async fn create_magic_link(&self, link: StatusPageMagicLink) -> Result<bool>;
    async fn consume_magic_link(
        &self,
        exchange: StatusPageMagicLinkExchange,
    ) -> Result<ConsumedStatusPageMagicLink>;
    async fn find_access_session(
        &self,
        page_id: &Id,
        token_hash: &str,
        origin_host: &str,
        now: TimestampMicros,
    ) -> Result<Option<StatusPageAccessSession>>;
    async fn list_access_sessions(
        &self,
        org_id: &Id,
        page_id: &Id,
        now: TimestampMicros,
    ) -> Result<Vec<StatusPageAccessSession>>;
    async fn revoke_access_session(
        &self,
        org_id: &Id,
        page_id: &Id,
        session_id: &Id,
        revoked_at: TimestampMicros,
    ) -> Result<()>;
    async fn revoke_all_access_sessions(
        &self,
        org_id: &Id,
        page_id: &Id,
        revoked_at: TimestampMicros,
    ) -> Result<u64>;
    async fn purge_expired_access_artifacts(&self, now: TimestampMicros, limit: u32)
    -> Result<u64>;
}
