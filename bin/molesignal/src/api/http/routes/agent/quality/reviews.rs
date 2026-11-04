// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 待审。
//!
//! - `GET  /api/v1/agent/quality/reviews?status=&before_at=&before_id=&limit=`
//! - `POST /api/v1/agent/quality/reviews/{id}/decision`：采纳（回答确实有问题，进入回归集）或驳回
//!
//! 审核人写的备注在检查 Prompt 时交给评审参考。

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    agent::scoring::{ReviewDecision, ReviewFilter, ReviewItem, ReviewStatus},
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::iam::permission,
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 一页最多这么多条。
const MAX_PAGE: i64 = 100;
const DEFAULT_PAGE: i64 = 50;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/quality/reviews", get(list))
        .route("/agent/quality/reviews/{id}/decision", post(decide))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListQuery {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    before_at: Option<i64>,
    #[serde(default)]
    before_id: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionInput {
    status: String,
    #[serde(default)]
    note: String,
}

#[permission("agent.quality.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>> {
    let status = query
        .status
        .as_deref()
        .filter(|status| !status.is_empty() && *status != "all")
        .map(ReviewStatus::parse)
        .transpose()?;

    let limit = query.limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE);

    let before = match (query.before_at, query.before_id) {
        (Some(at), Some(id)) => Some((TimestampMicros(at), Id(id))),
        _ => None,
    };

    let reviews: Vec<ReviewItem> = state
        .agent
        .scoring
        .list_reviews(
            &ctx.org_id,
            &ReviewFilter {
                status,
                before,
                limit,
            },
        )
        .await?;

    let next = (reviews.len() as i64 == limit)
        .then(|| reviews.last())
        .flatten()
        .map(|last| json!({ "beforeAt": last.review.created_at.0, "beforeId": last.review.id.0 }));

    Ok(Json(json!({ "reviews": reviews, "next": next })))
}

#[permission("agent.quality.manage")]
async fn decide(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(input): Json<DecisionInput>,
) -> Result<Json<Value>> {
    let decision = ReviewDecision {
        status: ReviewStatus::parse(&input.status)?,
        note: input.note,
        by: ctx.user_id.clone(),
        at: TimestampMicros::now(),
    }
    .normalized()?;

    let review = state
        .agent
        .scoring
        .decide_review(&ctx.org_id, &Id(id), &decision)
        .await?;

    // 审计只记决定，备注可能含业务细节，不进审计。
    activity_audit::record(
        &state,
        &ctx,
        "agent.quality.review_decided",
        "agent_score_review",
        &review.id.0,
        json!({
            "targetKind": review.target_kind,
            "targetId": review.target_id,
            "status": review.status,
            "hasNote": !review.note.is_empty(),
        }),
    )
    .await;

    Ok(Json(json!({ "review": review })))
}
