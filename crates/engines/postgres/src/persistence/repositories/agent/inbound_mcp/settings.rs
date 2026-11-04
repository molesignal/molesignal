// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 对外 MCP Server 的组织设置，存在 `org_settings` 的 `inbound_mcp` 一节。取值范围由设置接口校验。

use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use super::super::super::org_settings::{self, Section, sections::INBOUND_MCP};
use crate::{
    agent::inbound_mcp::InboundMcpSettings,
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// `inbound_mcp` 一节的值。`created_at` 是第一次保存的时间。
#[derive(Serialize, Deserialize)]
struct StoredSettings {
    enabled: bool,
    allowed_origins: Vec<String>,
    /// 一次请求的正文最大多少 MiB。
    max_request_size: i32,
    /// 一次响应最大多少 MiB。
    max_response_size: i32,
    max_concurrent_calls: i32,
    calls_per_minute: i32,
    read_timeout_ms: i64,
    created_at: TimestampMicros,
}

fn settings_of(org_id: &Id, section: Section<StoredSettings>) -> InboundMcpSettings {
    let stored = section.value;

    InboundMcpSettings {
        org_id: org_id.clone(),
        enabled: stored.enabled,
        allowed_origins: stored.allowed_origins,
        max_request_size: stored.max_request_size,
        max_response_size: stored.max_response_size,
        max_concurrent_calls: stored.max_concurrent_calls,
        calls_per_minute: stored.calls_per_minute,
        read_timeout_ms: stored.read_timeout_ms,
        updated_by: Id(section.updated_by.unwrap_or_default()),
        created_at: stored.created_at,
        updated_at: section.updated_at,
    }
}

pub(super) async fn get(pool: &PgPool, org_id: &Id) -> Result<Option<InboundMcpSettings>> {
    Ok(org_settings::read(pool, &org_id.0, INBOUND_MCP)
        .await?
        .map(|section| settings_of(org_id, section)))
}

pub(super) async fn upsert(
    pool: &PgPool,
    settings: InboundMcpSettings,
) -> Result<InboundMcpSettings> {
    // 第一次保存的时间不变。
    let created_at = org_settings::read::<StoredSettings>(pool, &settings.org_id.0, INBOUND_MCP)
        .await?
        .map_or(settings.created_at, |section| section.value.created_at);

    let stored = StoredSettings {
        enabled: settings.enabled,
        allowed_origins: settings.allowed_origins.clone(),
        max_request_size: settings.max_request_size,
        max_response_size: settings.max_response_size,
        max_concurrent_calls: settings.max_concurrent_calls,
        calls_per_minute: settings.calls_per_minute,
        read_timeout_ms: settings.read_timeout_ms,
        created_at,
    };

    org_settings::write(
        pool,
        &settings.org_id.0,
        INBOUND_MCP,
        &stored,
        Some(&settings.updated_by.0),
        settings.updated_at,
    )
    .await?;

    Ok(InboundMcpSettings {
        created_at,
        ..settings
    })
}
