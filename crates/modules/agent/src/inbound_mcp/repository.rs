// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use serde_json::Value;

use super::{
    InboundMcpAuthorizationCode, InboundMcpIdempotencyInput, InboundMcpIdempotencyReservation,
    InboundMcpOAuthClient, InboundMcpOAuthConnection, InboundMcpOAuthToken, InboundMcpSettings,
    InboundMcpTask, NewInboundMcpOAuthClient,
};
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[async_trait]
pub trait InboundMcpRepository: Send + Sync {
    async fn get_settings(&self, org_id: &Id) -> Result<Option<InboundMcpSettings>>;
    async fn upsert_settings(&self, settings: InboundMcpSettings) -> Result<InboundMcpSettings>;

    async fn create_oauth_client(
        &self,
        client: NewInboundMcpOAuthClient,
    ) -> Result<InboundMcpOAuthClient>;
    async fn get_oauth_client(&self, client_id: &str) -> Result<Option<InboundMcpOAuthClient>>;
    async fn list_oauth_clients(&self) -> Result<Vec<InboundMcpOAuthClient>>;
    async fn delete_oauth_client(&self, client_id: &str) -> Result<()>;

    async fn create_authorization_code(&self, code: InboundMcpAuthorizationCode) -> Result<()>;
    async fn consume_authorization_code(
        &self,
        code_hash: &str,
        client_id: &str,
        redirect_uri: &str,
        resource: &str,
        code_challenge: &str,
        now: TimestampMicros,
    ) -> Result<Option<InboundMcpAuthorizationCode>>;

    async fn create_oauth_token_pair(
        &self,
        access: InboundMcpOAuthToken,
        refresh: Option<InboundMcpOAuthToken>,
    ) -> Result<()>;
    async fn rotate_oauth_token_pair(
        &self,
        refresh_token_hash: &str,
        now: TimestampMicros,
        access: InboundMcpOAuthToken,
        refresh: Option<InboundMcpOAuthToken>,
    ) -> Result<bool>;
    async fn find_oauth_token(&self, token_hash: &str) -> Result<Option<InboundMcpOAuthToken>>;
    async fn list_oauth_tokens(&self, org_id: &Id) -> Result<Vec<InboundMcpOAuthToken>>;
    async fn list_oauth_connections(
        &self,
        org_id: &Id,
        now: TimestampMicros,
    ) -> Result<Vec<InboundMcpOAuthConnection>>;
    async fn revoke_oauth_token(&self, token_hash: &str, now: TimestampMicros) -> Result<()>;
    async fn revoke_oauth_family(
        &self,
        org_id: &Id,
        family_id: &Id,
        now: TimestampMicros,
    ) -> Result<()>;
    async fn revoke_oauth_client_tokens(
        &self,
        org_id: &Id,
        client_id: &str,
        now: TimestampMicros,
    ) -> Result<()>;

    async fn reserve_idempotency(
        &self,
        input: InboundMcpIdempotencyInput,
    ) -> Result<InboundMcpIdempotencyReservation>;
    async fn complete_idempotency(
        &self,
        input: &InboundMcpIdempotencyInput,
        approval_id: Option<&Id>,
        result: &Value,
        status: &str,
    ) -> Result<()>;

    async fn create_task(&self, task: InboundMcpTask) -> Result<InboundMcpTask>;
    async fn get_task(
        &self,
        org_id: &Id,
        principal_type: &str,
        principal_id: &Id,
        task_id: &Id,
    ) -> Result<InboundMcpTask>;
    async fn update_task(&self, task: InboundMcpTask) -> Result<InboundMcpTask>;
    async fn request_task_cancellation(
        &self,
        org_id: &Id,
        principal_type: &str,
        principal_id: &Id,
        task_id: &Id,
    ) -> Result<()>;
}
