// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET /api/v1/web/search?q=&types=&limit=` —— ⌘K 远端搜索聚合。
//!
//! 用 pg_trgm 在 streams / dashboards / saved_views / alert_rules /
//! incidents / service_graph_edges 6 张表上做 UNION ALL，按 `similarity` 排序。
//! 索引定义见 `src/infra/migrations/20260101000001_initial.sql`。
//!
//! Request / Response 类型分到 [`request`] / [`response`]，
//! handler 只做 IO 与组装。

use axum::{
    Extension, Router,
    extract::{Query, State},
    response::Json,
    routing::get,
};

use self::{
    request::SearchQuery,
    response::{SearchItem, SearchResponse},
};
use crate::{
    api::AppState,
    app::{iam::IamContext, web::search::validate_query},
    domain::iam::permission,
    shared::Error,
};

mod request;
mod response;

pub fn routes() -> Router<AppState> {
    Router::new().route("/search", get(search))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn search(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, Error> {
    // 空 q 短路返空，不当错误：⌘K 面板在用户还没输入时就会打这个端点，既有契约是 200 + []。
    if q.q.is_empty() {
        return Ok(Json(SearchResponse { items: Vec::new() }));
    }
    // 长度上限必须在打 DB 之前拦：下游是 6 表 UNION ALL 上的 pg_trgm 扫描，
    // 且那条查询的 statement_timeout 长期是失效的，没有别的兜底。
    validate_query(&q.q).map_err(Error::invalid)?;

    let limit = q.capped_limit();
    let kinds = q.parse_kinds();

    let hits = state
        .storage
        .web_search
        .search(&ctx.org_id, &q.q, &kinds, limit)
        .await?;
    let items = hits
        .into_iter()
        .map(|h| SearchItem {
            kind: h.kind,
            id: h.id,
            label: h.label,
            subtitle: h.subtitle,
        })
        .collect();
    Ok(Json(SearchResponse { items }))
}
