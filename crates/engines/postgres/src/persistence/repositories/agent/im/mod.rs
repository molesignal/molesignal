// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 在 IM 里：`agent_im_bots`、`agent_im_threads`、`agent_im_messages`、
//! `agent_im_outbox`，以及 `one_time_tokens` 里的绑定链接（`kind = 'im_link'`）。
//!
//! 机器人与它的租约在 [`bots`]，话题、消息与绑定链接在 [`messages`]，待发的回复在 [`outbox`]。

mod bots;
mod messages;
mod outbox;

use async_trait::async_trait;
use sqlx::{PgPool, Row, postgres::PgRow};

use super::super::sqlx_err;
use crate::{
    agent::im::{
        BotStatus, ImBot, ImLink, ImMessage, ImOutgoing, ImRepository, ImThread, MessageStatus,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgImRepository {
    pool: PgPool,
}

impl PgImRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn text(row: &PgRow, column: &str) -> Result<String> {
    row.try_get(column).map_err(sqlx_err)
}

fn optional_text(row: &PgRow, column: &str) -> Result<Option<String>> {
    row.try_get(column).map_err(sqlx_err)
}

fn micros(row: &PgRow, column: &str) -> Result<TimestampMicros> {
    row.try_get(column).map_err(sqlx_err)
}

fn optional_micros(row: &PgRow, column: &str) -> Result<Option<TimestampMicros>> {
    row.try_get(column).map_err(sqlx_err)
}

#[async_trait]
impl ImRepository for PgImRepository {
    async fn list_bots(&self, org_id: &Id) -> Result<Vec<ImBot>> {
        bots::list(&self.pool, Some(org_id), false).await
    }

    async fn get_bot(&self, org_id: &Id, connector_id: &Id) -> Result<Option<ImBot>> {
        bots::get(&self.pool, org_id, connector_id).await
    }

    async fn find_bot(&self, connector_id: &Id) -> Result<Option<ImBot>> {
        bots::find(&self.pool, connector_id).await
    }

    async fn put_bot(&self, bot: &ImBot) -> Result<ImBot> {
        bots::put(&self.pool, bot).await
    }

    async fn delete_bot(&self, org_id: &Id, connector_id: &Id) -> Result<()> {
        bots::delete(&self.pool, org_id, connector_id).await
    }

    async fn enabled_bots(&self) -> Result<Vec<ImBot>> {
        bots::list(&self.pool, None, true).await
    }

    async fn lease_bot(
        &self,
        connector_id: &Id,
        node: &str,
        now: TimestampMicros,
        until: TimestampMicros,
    ) -> Result<bool> {
        bots::lease(&self.pool, connector_id, node, now, until).await
    }

    async fn release_bot(&self, connector_id: &Id, node: &str) -> Result<()> {
        bots::release(&self.pool, connector_id, node).await
    }

    async fn set_bot_status(
        &self,
        connector_id: &Id,
        node: &str,
        status: BotStatus,
        detail: Option<&str>,
    ) -> Result<()> {
        bots::set_status(&self.pool, connector_id, node, status, detail).await
    }

    async fn touch_bot(&self, connector_id: &Id, at: TimestampMicros) -> Result<()> {
        bots::touch(&self.pool, connector_id, at).await
    }

    async fn thread(
        &self,
        connector_id: &Id,
        conversation_id: &str,
        thread_key: &str,
    ) -> Result<Option<ImThread>> {
        messages::thread(&self.pool, connector_id, conversation_id, thread_key).await
    }

    async fn put_thread(&self, thread: &ImThread) -> Result<ImThread> {
        messages::put_thread(&self.pool, thread).await
    }

    async fn record_message(&self, message: &ImMessage) -> Result<bool> {
        messages::record(&self.pool, message).await
    }

    async fn update_message(&self, message: &ImMessage) -> Result<()> {
        messages::update(&self.pool, message).await
    }

    async fn move_message(&self, id: &Id, from: MessageStatus, to: MessageStatus) -> Result<bool> {
        messages::move_status(&self.pool, id, from, to).await
    }

    async fn settle_message(
        &self,
        id: &Id,
        status: MessageStatus,
        detail: Option<&str>,
        replied_at: TimestampMicros,
    ) -> Result<()> {
        messages::settle(&self.pool, id, status, detail, replied_at).await
    }

    async fn message_for_run(&self, run_id: &Id) -> Result<Option<ImMessage>> {
        messages::for_run(&self.pool, run_id).await
    }

    async fn answering(&self, before: TimestampMicros, limit: i64) -> Result<Vec<ImMessage>> {
        messages::answering(&self.pool, before, limit).await
    }

    async fn purge_messages(&self, before: TimestampMicros) -> Result<u64> {
        outbox::purge(&self.pool, before).await?;
        messages::purge(&self.pool, before).await
    }

    async fn enqueue_outgoing(&self, outgoing: &ImOutgoing) -> Result<()> {
        outbox::enqueue(&self.pool, outgoing).await
    }

    async fn take_outgoing(&self, connector_id: &Id, limit: i64) -> Result<Vec<ImOutgoing>> {
        outbox::take(&self.pool, connector_id, limit).await
    }

    async fn create_link(&self, link: &ImLink) -> Result<()> {
        messages::create_link(&self.pool, link).await
    }

    async fn take_link(&self, token_hash: &str, now: TimestampMicros) -> Result<Option<ImLink>> {
        messages::take_link(&self.pool, token_hash, now).await
    }

    async fn peek_link(&self, token_hash: &str, now: TimestampMicros) -> Result<Option<ImLink>> {
        messages::peek_link(&self.pool, token_hash, now).await
    }
}
