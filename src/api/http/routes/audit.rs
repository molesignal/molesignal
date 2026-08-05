// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Audit activity routes（change `add-ai-anomaly-chat`：可搜索 + 游标分页）。
//!
//! `GET /api/v1/audit?from=&to=&actor_kind=&actor=&action=&target_kind=&target_id=&limit=&cursor=`
//! 返 `{ items, next_cursor }`，按 `ts DESC, id DESC` 排序；权限 `AuditRead`（Admin/Owner）。
//! `from` / `to` 支持绝对 micros 或相对串 `now` / `now-<N>{s,m,h,d}`。

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    infra::persistence::repositories::audit_events::{AuditEvent, AuditQuery},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/audit", get(query_audit))
        .route("/system/audit", get(query_system_audit))
        .route(
            "/intelligence/audit/chat/{id}",
            get(get_intelligence_chat_transcript),
        )
}

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;

#[derive(Debug, Deserialize)]
struct AuditQueryParams {
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    actor_kind: Option<String>,
    #[serde(default)]
    actor: Option<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    target_kind: Option<String>,
    #[serde(default)]
    target_id: Option<String>,
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    cursor: Option<String>,
}

fn default_limit() -> i64 {
    DEFAULT_LIMIT
}

#[derive(Debug, Serialize)]
struct Resp {
    id: String,
    org_id: String,
    actor_kind: String,
    actor_id: String,
    action: String,
    target_kind: Option<String>,
    target_id: Option<String>,
    ip: Option<String>,
    user_agent: Option<String>,
    payload: Value,
    ts_micros: i64,
}

fn to_resp(event: AuditEvent) -> Resp {
    Resp {
        id: event.id.0,
        org_id: event.org_id.0,
        actor_kind: event.actor_kind,
        actor_id: event.actor_id,
        action: event.action,
        target_kind: event.target_kind,
        target_id: event.target_id,
        ip: event.ip,
        user_agent: event.user_agent,
        payload: event.payload,
        ts_micros: event.ts.0,
    }
}

#[derive(Debug, Serialize)]
struct AuditPage {
    items: Vec<Resp>,
    next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
struct AuditChatResp {
    id: String,
    provider: String,
    model: String,
    title: String,
    provider_id: Option<String>,
    analysis_mode: Option<String>,
    time_range_start_micros: Option<i64>,
    time_range_end_micros: Option<i64>,
    archive_object_key: Option<String>,
    deleted_at_micros: Option<i64>,
    created_at_micros: i64,
    updated_at_micros: i64,
}

#[derive(Debug, Serialize)]
struct AuditChatMessageResp {
    id: String,
    chat_id: String,
    org_id: String,
    role: String,
    content: String,
    prompt_template_id: Option<String>,
    prompt_builtin_key: Option<String>,
    prompt_version: Option<i32>,
    prompt_hash: Option<String>,
    evidence_json: Option<Value>,
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    cost_usd: Option<f64>,
    created_at_micros: i64,
}

#[derive(Debug, Serialize)]
struct AuditChatTranscriptResp {
    chat: AuditChatResp,
    messages: Vec<AuditChatMessageResp>,
}

/// 解析时间：绝对 micros 整数，或相对串 `now` / `now-<N>{s,m,h,d}`。
fn parse_time(s: &str) -> Result<i64> {
    let s = s.trim();
    if let Ok(n) = s.parse::<i64>() {
        return Ok(n);
    }
    if s == "now" {
        return Ok(TimestampMicros::now().0);
    }
    if let Some(rest) = s.strip_prefix("now-") {
        if rest.len() < 2 {
            return Err(Error::invalid(format!("bad relative time: {s}")));
        }
        let (num, unit) = rest.split_at(rest.len() - 1);
        let n: i64 = num
            .parse()
            .map_err(|_| Error::invalid(format!("bad relative time: {s}")))?;
        let mult = match unit {
            "s" => 1_000_000,
            "m" => 60_000_000,
            "h" => 3_600_000_000,
            "d" => 86_400_000_000,
            _ => return Err(Error::invalid(format!("bad time unit in: {s}"))),
        };
        return Ok(TimestampMicros::now().0 - n * mult);
    }
    Err(Error::invalid(format!("unparseable time: {s}")))
}

/// 游标 = base64url("{ts_micros}:{id}")；对客户端不透明。
fn encode_cursor(ts: i64, id: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!("{ts}:{id}"))
}

fn decode_cursor(s: &str) -> Result<(i64, String)> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s.trim())
        .map_err(|_| Error::invalid("invalid cursor"))?;
    let text = String::from_utf8(bytes).map_err(|_| Error::invalid("invalid cursor"))?;
    let (ts, id) = text
        .split_once(':')
        .ok_or_else(|| Error::invalid("invalid cursor"))?;
    let ts: i64 = ts.parse().map_err(|_| Error::invalid("invalid cursor"))?;
    Ok((ts, id.to_string()))
}

#[permission("audit.read")]
async fn query_audit(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<AuditQueryParams>,
) -> Result<Json<AuditPage>> {
    // Admin / Owner only（Viewer / Editor → 403）。
    let page_size = params.limit.clamp(1, MAX_LIMIT);
    let q = AuditQuery {
        from_micros: params.from.as_deref().map(parse_time).transpose()?,
        to_micros: params.to.as_deref().map(parse_time).transpose()?,
        actor_kind: params.actor_kind.filter(|s| !s.is_empty()),
        actor_id: params.actor.filter(|s| !s.is_empty()),
        action: params.action.filter(|s| !s.is_empty()),
        target_kind: params.target_kind.filter(|s| !s.is_empty()),
        target_id: params.target_id.filter(|s| !s.is_empty()),
        // 多取一行探测是否有下一页。
        limit: page_size + 1,
        cursor: params.cursor.as_deref().map(decode_cursor).transpose()?,
    };

    let mut rows = state.iam.audit_events.query(&ctx.org_id, &q).await?;
    let next_cursor = if rows.len() as i64 > page_size {
        let last = &rows[(page_size - 1) as usize];
        let c = encode_cursor(last.ts.0, &last.id.0);
        rows.truncate(page_size as usize);
        Some(c)
    } else {
        None
    };

    Ok(Json(AuditPage {
        items: rows.into_iter().map(to_resp).collect(),
        next_cursor,
    }))
}

#[permission("sys.telemetry.read")]
async fn query_system_audit(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<AuditQueryParams>,
) -> Result<Json<AuditPage>> {
    query_audit_for_org(&state, &state.iam.system_org_id, params).await
}

async fn query_audit_for_org(
    state: &AppState,
    org_id: &Id,
    params: AuditQueryParams,
) -> Result<Json<AuditPage>> {
    let page_size = params.limit.clamp(1, MAX_LIMIT);
    let query = AuditQuery {
        from_micros: params.from.as_deref().map(parse_time).transpose()?,
        to_micros: params.to.as_deref().map(parse_time).transpose()?,
        actor_kind: params.actor_kind.filter(|value| !value.is_empty()),
        actor_id: params.actor.filter(|value| !value.is_empty()),
        action: params.action.filter(|value| !value.is_empty()),
        target_kind: params.target_kind.filter(|value| !value.is_empty()),
        target_id: params.target_id.filter(|value| !value.is_empty()),
        limit: page_size + 1,
        cursor: params.cursor.as_deref().map(decode_cursor).transpose()?,
    };
    let mut rows = state.iam.audit_events.query(org_id, &query).await?;
    let next_cursor = if rows.len() as i64 > page_size {
        let last = &rows[(page_size - 1) as usize];
        let cursor = encode_cursor(last.ts.0, &last.id.0);
        rows.truncate(page_size as usize);
        Some(cursor)
    } else {
        None
    };
    Ok(Json(AuditPage {
        items: rows.into_iter().map(to_resp).collect(),
        next_cursor,
    }))
}

#[permission("audit.read")]
async fn get_intelligence_chat_transcript(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<AuditChatTranscriptResp>> {
    let chat_id = Id(id);
    let chat = state
        .intelligence
        .chats
        .get_chat_any(&ctx.org_id, &chat_id)
        .await?;
    let messages = state.intelligence.chats.list_messages(&chat.id).await?;

    Ok(Json(AuditChatTranscriptResp {
        chat: AuditChatResp {
            id: chat.id.0,
            provider: chat.provider,
            model: chat.model,
            title: chat.title,
            provider_id: chat.provider_id,
            analysis_mode: chat.analysis_mode,
            time_range_start_micros: chat.time_range_start_micros,
            time_range_end_micros: chat.time_range_end_micros,
            archive_object_key: chat.archive_object_key,
            deleted_at_micros: chat.deleted_at_micros,
            created_at_micros: chat.created_at.0,
            updated_at_micros: chat.updated_at.0,
        },
        messages: messages
            .into_iter()
            .map(|m| AuditChatMessageResp {
                id: m.id.0,
                chat_id: m.chat_id.0,
                org_id: m.org_id.0,
                role: m.role,
                content: m.content,
                prompt_template_id: m.prompt_template_id,
                prompt_builtin_key: m.prompt_builtin_key,
                prompt_version: m.prompt_version,
                prompt_hash: m.prompt_hash,
                evidence_json: m.evidence_json,
                prompt_tokens: m.prompt_tokens,
                completion_tokens: m.completion_tokens,
                cost_usd: m.cost_usd,
                created_at_micros: m.created_at.0,
            })
            .collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_time_absolute_and_relative() {
        assert_eq!(
            parse_time("1700000000000000").unwrap(),
            1_700_000_000_000_000
        );
        let now = TimestampMicros::now().0;
        let an_hour_ago = parse_time("now-1h").unwrap();
        assert!((now - an_hour_ago - 3_600_000_000).abs() < 5_000_000);
        assert!(parse_time("now-3x").is_err());
        assert!(parse_time("garbage").is_err());
    }

    #[test]
    fn cursor_roundtrips() {
        let c = encode_cursor(1_700_000_000_000_000, "evt-123");
        let (ts, id) = decode_cursor(&c).unwrap();
        assert_eq!(ts, 1_700_000_000_000_000);
        assert_eq!(id, "evt-123");
        // opaque：不是明文。
        assert!(!c.contains(':'));
        assert!(decode_cursor("not-base64!!").is_err());
    }
}
