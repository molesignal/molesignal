// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET /agent/runs/{id}/events`：从某个序号之后续读 Run 的事件（SSE）。
//!
//! 先把库里已有的发完，再等提醒：同一节点写的直接提醒，别的节点写的经 NOTIFY 转来，没有提醒时
//! 每 15 秒发一行注释保持连接并查一次 Run。读到结束事件就关。Run 早已结束、事件已被清理时，
//! 按 Run 上记下的结果补一个结束事件。每个事件带 `id:`，断开后用 `Last-Event-ID` 或
//! `?after=` 接着读。

use std::{convert::Infallible, time::Duration};

use axum::{
    Extension,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, header::CONTENT_TYPE},
    response::Response,
};
use bytes::Bytes;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc::Sender;

use super::readable_run;
use crate::{
    agent::runs::{Run, RunEvent, RunEventKind},
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    shared::{Result, ids::Id, trace_stream::segmented_result_stream},
};

/// 一次最多从库里读这么多个事件。
const PAGE: i64 = 500;
/// 没有新事件时多久发一行注释、查一次 Run。
const KEEPALIVE: Duration = Duration::from_secs(15);

type Frame = std::result::Result<Bytes, Infallible>;

#[derive(Debug, Deserialize)]
pub(super) struct EventsQuery {
    #[serde(default)]
    after: Option<i64>,
}

#[permission("agent.use")]
pub(super) async fn events(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Query(query): Query<EventsQuery>,
    headers: HeaderMap,
) -> Result<Response> {
    let run = readable_run(&state, &ctx, &id).await?;

    let resume = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<i64>().ok());

    let after = resume.or(query.after).unwrap_or(0).max(0);
    let (tx, rx) = tokio::sync::mpsc::channel::<Frame>(64);

    crate::shared::trace_context::spawn_with_current_trace_context(relay(state, run, after, tx));

    let frames = Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx));
    let body = segmented_result_stream(frames, "agent.http.sse", "sse");

    Ok(Response::builder()
        .status(200)
        .header(CONTENT_TYPE, "text/event-stream")
        .header("cache-control", "no-cache")
        .header("x-accel-buffering", "no")
        .body(Body::from_stream(body))
        .unwrap_or_else(|_| Response::new(Body::empty())))
}

fn frame(event: &RunEvent) -> Frame {
    Ok(Bytes::from(format!(
        "id: {}\nevent: {}\ndata: {}\n\n",
        event.seq,
        event.kind.as_str(),
        event.payload
    )))
}

/// 发出这些事件；读到结束事件或客户端走了返回 false。
async fn send(tx: &Sender<Frame>, events: &[RunEvent]) -> bool {
    for event in events {
        if tx.send(frame(event)).await.is_err() || event.kind == RunEventKind::Done {
            return false;
        }
    }

    true
}

async fn relay(state: AppState, run: Run, mut seq: i64, tx: Sender<Frame>) {
    let runs = &state.agent.runs;
    let mut signals = runs.signals();

    loop {
        let events = match runs.events_after(&run.id, seq, PAGE).await {
            Ok(events) => events,
            Err(error) => {
                tracing::warn!(run_id = %run.id.0, %error, "agent run events could not be read");
                return;
            }
        };

        if let Some(last) = events.last() {
            seq = last.seq;
        }

        if !send(&tx, &events).await {
            return;
        }

        if !events.is_empty() {
            continue;
        }

        // 没有新事件：Run 结束了就把剩下的读完，否则等。
        let current = match runs.get(&run.org_id, &run.id).await {
            Ok(current) => current,
            Err(_) => return,
        };

        if current.status.is_terminal() {
            finish(&state, &current, seq, &tx).await;
            return;
        }

        if !signals.changed(&run.id, KEEPALIVE).await
            && tx
                .send(Ok(Bytes::from_static(b": keepalive\n\n")))
                .await
                .is_err()
        {
            return;
        }
    }
}

/// Run 已经结束：读完剩下的事件；事件已被清理时按 Run 上的结果补一个结束事件。
async fn finish(state: &AppState, run: &Run, seq: i64, tx: &Sender<Frame>) {
    if seq >= run.last_seq {
        return;
    }

    let rest = state
        .agent
        .runs
        .events_after(&run.id, seq, PAGE)
        .await
        .unwrap_or_default();

    if !rest.is_empty() {
        send(tx, &rest).await;
        return;
    }

    let done = RunEvent {
        run_id: Id(run.id.0.clone()),
        seq: run.last_seq,
        kind: RunEventKind::Done,
        payload: json!({
            "status": run.status.as_str(),
            "message_id": run.assistant_message_id.as_ref().map(|id| id.0.clone()),
            "prompt_tokens": run.prompt_tokens,
            "completion_tokens": run.completion_tokens,
            "cost": run.cost,
            "error": run.error,
        }),
        created_at: run.finished_at.unwrap_or(run.updated_at),
    };

    let _ = tx.send(frame(&done)).await;
}
