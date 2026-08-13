// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PostgreSQL adapter for the Inbound MCP control plane.

use async_trait::async_trait;
use serde_json::Value;
use sqlx::PgPool;

use crate::{
    agent::inbound_mcp::{
        InboundMcpAuthorizationCode, InboundMcpIdempotencyInput, InboundMcpIdempotencyReservation,
        InboundMcpOAuthClient, InboundMcpOAuthConnection, InboundMcpOAuthToken,
        InboundMcpRepository, InboundMcpSettings, InboundMcpTask, NewInboundMcpOAuthClient,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

mod idempotency;
mod oauth;
mod settings;
mod tasks;

pub struct PgInboundMcpRepository {
    pool: PgPool,
}

impl PgInboundMcpRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl InboundMcpRepository for PgInboundMcpRepository {
    async fn get_settings(&self, org_id: &Id) -> Result<Option<InboundMcpSettings>> {
        settings::get(&self.pool, org_id).await
    }

    async fn upsert_settings(&self, settings: InboundMcpSettings) -> Result<InboundMcpSettings> {
        settings::upsert(&self.pool, settings).await
    }

    async fn create_oauth_client(
        &self,
        client: NewInboundMcpOAuthClient,
    ) -> Result<InboundMcpOAuthClient> {
        oauth::create_client(&self.pool, client).await
    }

    async fn get_oauth_client(&self, client_id: &str) -> Result<Option<InboundMcpOAuthClient>> {
        oauth::get_client(&self.pool, client_id).await
    }

    async fn list_oauth_clients(&self) -> Result<Vec<InboundMcpOAuthClient>> {
        oauth::list_clients(&self.pool).await
    }

    async fn delete_oauth_client(&self, client_id: &str) -> Result<()> {
        oauth::delete_client(&self.pool, client_id).await
    }

    async fn create_authorization_code(&self, code: InboundMcpAuthorizationCode) -> Result<()> {
        oauth::create_code(&self.pool, code).await
    }

    async fn consume_authorization_code(
        &self,
        code_hash: &str,
        client_id: &str,
        redirect_uri: &str,
        resource: &str,
        code_challenge: &str,
        now: TimestampMicros,
    ) -> Result<Option<InboundMcpAuthorizationCode>> {
        oauth::consume_code(
            &self.pool,
            code_hash,
            client_id,
            redirect_uri,
            resource,
            code_challenge,
            now,
        )
        .await
    }

    async fn create_oauth_token_pair(
        &self,
        access: InboundMcpOAuthToken,
        refresh: Option<InboundMcpOAuthToken>,
    ) -> Result<()> {
        oauth::create_token_pair(&self.pool, access, refresh).await
    }

    async fn rotate_oauth_token_pair(
        &self,
        refresh_token_hash: &str,
        now: TimestampMicros,
        access: InboundMcpOAuthToken,
        refresh: Option<InboundMcpOAuthToken>,
    ) -> Result<bool> {
        oauth::rotate_token_pair(&self.pool, refresh_token_hash, now, access, refresh).await
    }

    async fn find_oauth_token(&self, token_hash: &str) -> Result<Option<InboundMcpOAuthToken>> {
        oauth::find_token(&self.pool, token_hash).await
    }

    async fn list_oauth_tokens(&self, org_id: &Id) -> Result<Vec<InboundMcpOAuthToken>> {
        oauth::list_tokens(&self.pool, org_id).await
    }

    async fn list_oauth_connections(
        &self,
        org_id: &Id,
        now: TimestampMicros,
    ) -> Result<Vec<InboundMcpOAuthConnection>> {
        oauth::list_connections(&self.pool, org_id, now).await
    }

    async fn revoke_oauth_token(&self, token_hash: &str, now: TimestampMicros) -> Result<()> {
        oauth::revoke_token(&self.pool, token_hash, now).await
    }

    async fn revoke_oauth_family(
        &self,
        org_id: &Id,
        family_id: &Id,
        now: TimestampMicros,
    ) -> Result<()> {
        oauth::revoke_family(&self.pool, org_id, family_id, now).await
    }

    async fn revoke_oauth_client_tokens(
        &self,
        org_id: &Id,
        client_id: &str,
        now: TimestampMicros,
    ) -> Result<()> {
        oauth::revoke_client_tokens(&self.pool, org_id, client_id, now).await
    }

    async fn reserve_idempotency(
        &self,
        input: InboundMcpIdempotencyInput,
    ) -> Result<InboundMcpIdempotencyReservation> {
        idempotency::reserve(&self.pool, input).await
    }

    async fn complete_idempotency(
        &self,
        input: &InboundMcpIdempotencyInput,
        approval_id: Option<&Id>,
        result: &Value,
        status: &str,
    ) -> Result<()> {
        idempotency::complete(&self.pool, input, approval_id, result, status).await
    }

    async fn create_task(&self, task: InboundMcpTask) -> Result<InboundMcpTask> {
        tasks::create(&self.pool, task).await
    }

    async fn get_task(
        &self,
        org_id: &Id,
        principal_type: &str,
        principal_id: &Id,
        task_id: &Id,
    ) -> Result<InboundMcpTask> {
        tasks::get(&self.pool, org_id, principal_type, principal_id, task_id).await
    }

    async fn update_task(&self, task: InboundMcpTask) -> Result<InboundMcpTask> {
        tasks::update(&self.pool, task).await
    }

    async fn request_task_cancellation(
        &self,
        org_id: &Id,
        principal_type: &str,
        principal_id: &Id,
        task_id: &Id,
    ) -> Result<()> {
        tasks::request_cancellation(&self.pool, org_id, principal_type, principal_id, task_id).await
    }
}
