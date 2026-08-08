// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{
        HeaderMap,
        header::{ACCEPT, CONTENT_TYPE},
    },
    response::Response,
    routing::{get, post},
};
use futures::stream::{self, StreamExt};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    api::AppState,
    app::{iam::IamContext, query::ActiveQuerySnapshot},
    domain::{
        iam::permission,
        query::{QueryLanguage, QueryRequest, QueryResult, StreamHint},
    },
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
        trace_stream::segmented_result_stream,
    },
};

mod prometheus;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/query", post(execute_query))
        .route("/query/stream", get(stream_query_get))
        .route("/query/stream", post(stream_query_post))
        .route("/query/search_around", post(search_around))
        .route("/query/inspect", post(inspect_query))
        .route("/query/recommendations", post(query_recommendations))
        .route("/query/promql/capabilities", get(promql_capabilities))
        .route("/query/sql/capabilities", get(sql_capabilities))
        .route("/query/slow", get(list_slow_queries))
        .route(
            "/query/admission",
            get(admission_stats).put(update_admission),
        )
        .route("/query/running", get(list_running))
        .route("/query/{id}/cancel", post(cancel_query))
        .merge(prometheus::routes())
}

/// Returns the static PromQL functions, aggregations and operators implemented
/// by the active MoleSignal engine. The endpoint intentionally contains no
/// organization data and is public so offline editor sessions can still
/// provide engine-compatible completions.
async fn promql_capabilities() -> Json<crate::infra::query::promql::capabilities::PromqlCapabilities>
{
    Json(crate::infra::query::promql::capabilities::capabilities())
}

/// Returns the static SQL text-search functions (`MATCH` / `MATCH_TEXT`) the
/// engine rewrites in `extract_match_predicates`. Same contract as the PromQL
/// capabilities endpoint: it contains no organization data and therefore does
/// not require an additional product permission beyond the API auth boundary.
async fn sql_capabilities() -> Json<crate::infra::query::sql_functions::SqlQueryCapabilities> {
    Json(crate::infra::query::sql_functions::sql_query_capabilities())
}

/// 查询优化建议：对给定查询画像（语句 + 上一次执行统计）做启发式分析，返回顾问性建议。
/// 无状态、不执行查询；统计通常由前端从上一次查询响应（scanned_rows/took_ms）带入。
#[permission(any("streams.query", "sys.telemetry.read"))]
async fn query_recommendations(
    Extension(ctx): Extension<IamContext>,
    Json(profile): Json<crate::app::recommendations::QueryProfile>,
) -> Result<Json<Value>> {
    let recommendations = crate::app::recommendations::analyze(&profile);
    Ok(Json(
        serde_json::json!({ "recommendations": recommendations }),
    ))
}

/// spec search-inspector：返回查询元数据 + 估算成本，不真正执行。
/// 当前实装：返 statement + time_range + stream + 阶段时间预算 `meta.profile`。
/// 完整 DataFusion logical+physical plan dump 留 follow-up（需暴露 planner API）。
#[permission(any("streams.query", "sys.telemetry.read"))]
async fn inspect_query(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(mut req): Json<QueryRequest>,
) -> Result<Json<Value>> {
    let started = std::time::Instant::now();
    req.org_id = ctx.org_id.clone();
    // 优化后逻辑计划（schema-only 规划，不执行）；规划失败时降级为提示文本而非整体失败。
    let logical_plan = match state.query.explain(req.clone()).await {
        Ok(p) => p,
        Err(e) => format!("<plan unavailable: {e}>"),
    };
    let stream_name = req
        .stream
        .as_ref()
        .map(|s| s.name.clone())
        .unwrap_or_default();

    // 提取 FROM 表（仅文档/前端显示用；走 sqlparser AST，CTE 内层会被正确归到 base table）
    let referenced_tables = parse_from_tables(&req.statement);
    // 解析失败 → 空 vec（保持 inspect 即便 SQL 不合法也能返一些上下文）。
    // 时间窗（秒）— 给前端做 cost 大致预估
    let window_secs = (req.time_range.end.0 - req.time_range.start.0).max(0) / 1_000_000;
    let parse_ms = started.elapsed().as_micros() as f64 / 1000.0;

    Ok(Json(serde_json::json!({
        "executed": false,
        "language": req.language,
        "statement": req.statement,
        "time_range": {
            "start": req.time_range.start.0,
            "end": req.time_range.end.0,
            "window_secs": window_secs,
        },
        "stream": stream_name,
        "tables": referenced_tables,
        "logical_plan": logical_plan,
        "physical_plan": "<not yet implemented>",
        "estimated_cost": null,
        "meta": {
            "profile": {
                "stages": [
                    { "name": "parse_sql", "ms": parse_ms, "estimated": false },
                    { "name": "parquet_file_meta_scan", "ms": null, "estimated": true,
                      "note": "depends on time_range + stream cardinality" },
                    { "name": "object_store_get", "ms": null, "estimated": true },
                    { "name": "datafusion_execute", "ms": null, "estimated": true }
                ],
                "elapsed_ms": parse_ms,
            }
        },
    })))
}

/// 抓取 SQL 中所有 base table 名（UI inspect 用）。
///
/// 走 `crate::infra::query::parser::extract_referenced_tables` 的 sqlparser
/// AST walker（spec query / sqlparser-join-planner change）：
/// - CTE alias 不再被误抓
/// - quoted identifier 自动去引号
/// - schema-qualified 收敛到表名
///
/// 解析失败（用户写了不合法 SQL）→ 返空 vec，让 inspect endpoint 仍能返其它上下文。
fn parse_from_tables(stmt: &str) -> Vec<String> {
    crate::infra::query::parser::extract_referenced_tables(stmt)
        .map(|refs| refs.into_iter().map(|r| r.name).collect())
        .unwrap_or_default()
}

/// 基于时间窗 + 单位窗口吞吐估算 rows，超阈值 → auto-async。
/// 0 阈值表示禁用。
fn should_auto_async(req: &QueryRequest, state: &AppState) -> bool {
    // AppState 暂未保存 QuerierSettings；通过 config 单例读取
    let cfg = crate::config::get();
    let threshold = cfg.querier.auto_async_threshold_rows;
    if threshold == 0 {
        return false;
    }
    let throughput = cfg.querier.estimate_throughput_per_sec.max(1);
    let window_secs = (req.time_range.end.0 - req.time_range.start.0).max(0) / 1_000_000;
    let estimated_rows = (window_secs as u64).saturating_mul(throughput);
    let _ = state; // 保留参数以便接入真实 parquet_file_meta 估算
    estimated_rows >= threshold
}

/// `?clusters=<csv>` —— 联邦查询目标集群（spec federated-search）。
/// 省略或仅 `"local"` 表示纯本地查询；其它为 `remote_clusters` 注册的远端 id/name。
#[derive(Debug, Deserialize)]
pub struct ClustersParam {
    #[serde(default)]
    pub clusters: Option<String>,
}

/// 把 `?clusters=` csv 解析进 `req.federation_clusters`，并在指向非 `local` 远端时
/// 施加 `federated_search` license 闸门（federated-search，社区版返 403）。
/// 纯本地查询（空 / 仅 local）不受 license 限制。
/// 解析 `?clusters=` csv → 集群名列表（去空白、去空项）。`None`/空串 → 空 vec。
fn parse_clusters_csv(raw: Option<&str>) -> Vec<String> {
    raw.map(|s| {
        s.split(',')
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
    })
    .unwrap_or_default()
}

fn apply_federation_clusters(
    req: &mut QueryRequest,
    cq: &ClustersParam,
    state: &AppState,
) -> Result<()> {
    req.federation_clusters = parse_clusters_csv(cq.clusters.as_deref());
    let has_remote = req
        .federation_clusters
        .iter()
        .any(|c| !c.eq_ignore_ascii_case("local"));
    if has_remote && !state.platform.license.has_feature("federated_search") {
        return Err(Error::forbidden(
            "feature 'federated_search' requires license",
        ));
    }
    Ok(())
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn execute_query(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(cq): Query<ClustersParam>,
    headers: HeaderMap,
    Json(mut req): Json<QueryRequest>,
) -> Result<Response> {
    req.org_id = ctx.org_id.clone();
    apply_federation_clusters(&mut req, &cq, &state)?;

    // spec query mod：`Prefer: respond-async` 头 → 转 async search-job，返 202；
    // 或：planner 估算 rows 超 `auto_async_threshold_rows` 也强制 async（除非
    // 请求显式 `Prefer: respond-sync`）。
    let prefer_str = headers.get("prefer").and_then(|v| v.to_str().ok());
    let prefer_async_explicit = prefer_str.is_some_and(|s| s.contains("respond-async"));
    let prefer_sync_explicit = prefer_str.is_some_and(|s| s.contains("respond-sync"));
    let auto_async =
        !prefer_sync_explicit && should_auto_async(&req, &state) && !prefer_async_explicit;
    let prefer_async = prefer_async_explicit || auto_async;
    if prefer_async {
        use crate::infra::persistence::repositories::search::jobs::{SearchJob, SearchJobState};
        let now = TimestampMicros::now();
        let ttl_secs: i64 = 7 * 86400;
        let job = SearchJob {
            id: Id::new(),
            org_id: ctx.org_id.clone(),
            user_id: ctx.user_id.clone(),
            request_json: serde_json::to_value(&req)
                .map_err(|e| Error::internal(format!("request json: {e}")))?,
            trace_link: crate::shared::trace_context::current_trace_context()
                .map(|context| context.serialized_link()),
            state: SearchJobState::Pending,
            result_object_key: None,
            result_rows: None,
            error: None,
            submitted_at: now,
            started_at: None,
            finished_at: None,
            expires_at: TimestampMicros(now.0 + ttl_secs * 1_000_000),
        };
        let job = state.storage.search_jobs.create(job).await?;
        let body = serde_json::json!({
            "job_id": job.id.0,
            "monitor": format!("/api/v1/query/jobs/{}", job.id.0),
            "state": "pending",
        });
        return Response::builder()
            .status(202)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .map_err(|e| Error::internal(format!("response build: {e}")));
    }

    // Accept: application/x-ndjson → 流式 NDJSON 输出，bypass query_result cache
    let streaming = headers
        .get(ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.contains("application/x-ndjson"));

    if streaming {
        // 当前：调一次 query.run，按 row chunk Body::from_stream 输出 NDJSON。
        // 真正的 RecordBatch streaming 待 QueryEngine 增 streaming API 后再上。
        return Ok(ndjson_response(
            state
                .query
                .run_tracked(req, ctx.user_id.clone(), ctx.organization_role_key())
                .await?,
        ));
    }

    // 捕获慢查询采集所需字段（req 随后被 run_tracked 消费）。仅同步路径采集；
    // 流式（x-ndjson）路径走前面的 early return，不采集。
    let cap_stmt = req.statement.clone();
    let cap_lang = req.language;
    let cap_range_secs = (req.time_range.end.0 - req.time_range.start.0).max(0) / 1_000_000;
    let out = state
        .query
        .run_tracked(req, ctx.user_id.clone(), ctx.organization_role_key())
        .await?;
    maybe_record_slow_query(
        &state,
        &ctx.org_id,
        cap_lang,
        cap_stmt,
        cap_range_secs,
        &out,
    );
    Response::builder()
        .status(200)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&out).unwrap_or_default()))
        .map_err(|e| Error::internal(format!("response build: {e}")))
}

/// 慢查询采集的捕获阈值（ms）：低于 engine 的 `slow_query` 规则阈值，以便提前留存
/// 临界慢查询供分析。
const SLOW_QUERY_CAPTURE_MS: u64 = 1_000;

/// 查询超阈值时 best-effort 记录慢查询（异步、不阻塞响应、失败仅 warn）。
fn maybe_record_slow_query(
    state: &AppState,
    org_id: &Id,
    language: QueryLanguage,
    statement: String,
    time_range_secs: i64,
    out: &QueryResult,
) {
    if out.took_ms < SLOW_QUERY_CAPTURE_MS {
        return;
    }
    let repo = state.intelligence.slow_queries.clone();
    let now = TimestampMicros::now();
    let row = crate::domain::query::SlowQuery {
        id: Id::new(),
        org_id: org_id.clone(),
        fingerprint: crate::app::recommendations::query_fingerprint(language, &statement),
        language,
        statement,
        scanned_rows: out.scanned_rows as i64,
        returned_rows: out.rows.len() as i64,
        took_ms: out.took_ms as i64,
        time_range_secs: Some(time_range_secs),
        hit_count: 1,
        first_seen: now,
        last_seen: now,
    };
    crate::shared::trace_context::spawn_with_current_trace_context(async move {
        if let Err(e) = repo.record(row).await {
            tracing::warn!(error = %e, "record slow query failed");
        }
    });
}

/// Search admission 各工作组的并发槽位快照（in-flight / limit / 累计申请/拒绝）。
#[permission("org.settings.read")]
async fn admission_stats(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let groups = state.query.admission().stats();
    Ok(Json(serde_json::json!({ "groups": groups })))
}

#[derive(Debug, Deserialize)]
struct AdmissionUpdateReq {
    #[serde(default)]
    default_max_concurrent: usize,
    #[serde(default)]
    groups: std::collections::HashMap<String, usize>,
    #[serde(default)]
    role_map: std::collections::HashMap<String, String>,
    #[serde(default)]
    cluster_default_max_concurrent: usize,
    #[serde(default)]
    cluster_groups: std::collections::HashMap<String, usize>,
}

/// 热更新 admission 配置（无需重启；仅本节点、不持久化——重启回落配置文件）。
/// 需要 `org.settings.manage`。
#[permission("org.settings.manage")]
async fn update_admission(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<AdmissionUpdateReq>,
) -> Result<Json<Value>> {
    state
        .query
        .admission()
        .refresh(crate::app::search::AdmissionConfig {
            default_max_concurrent: req.default_max_concurrent,
            groups: req.groups,
            role_map: req.role_map,
            cluster_default_max_concurrent: req.cluster_default_max_concurrent,
            cluster_groups: req.cluster_groups,
        });
    let groups = state.query.admission().stats();
    Ok(Json(serde_json::json!({ "groups": groups })))
}

/// 最近慢查询 + 各自的优化建议（按需在读路径用启发式引擎计算）。
#[permission(any("streams.query", "sys.telemetry.read"))]
async fn list_slow_queries(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let rows = state
        .intelligence
        .slow_queries
        .list_recent(&ctx.org_id, 50)
        .await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|sq| {
            let recommendations = crate::app::recommendations::analyze(
                &crate::app::recommendations::QueryProfile::from(sq),
            );
            serde_json::json!({ "slow_query": sq, "recommendations": recommendations })
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

/// 按行 chunk 把 `QueryResult` 流式输出为 NDJSON，末尾追加 `__meta__` 行。
///
/// 现阶段 [`QueryService::run`] 仍同步返回全部 rows；用 `Body::from_stream` 套上
/// stream 可让 axum 走 chunked encoding，对 ingress (`proxy_buffering: off`) 友好。
fn ndjson_response(out: QueryResult) -> Response {
    let columns = out.columns;
    let scanned_rows = out.scanned_rows;
    let took_ms = out.took_ms;
    let row_iter = out.rows.into_iter().map(move |row| {
        let mut obj = serde_json::Map::with_capacity(columns.len());
        for (i, col) in columns.iter().enumerate() {
            obj.insert(col.clone(), row.get(i).cloned().unwrap_or(Value::Null));
        }
        let mut line = serde_json::to_string(&Value::Object(obj)).unwrap_or_default();
        line.push('\n');
        Ok::<_, std::convert::Infallible>(bytes::Bytes::from(line))
    });
    let trailer = {
        let mut s = serde_json::to_string(&serde_json::json!({
            "__meta__": { "scanned_rows": scanned_rows, "took_ms": took_ms }
        }))
        .unwrap_or_default();
        s.push('\n');
        Ok::<_, std::convert::Infallible>(bytes::Bytes::from(s))
    };
    let body_stream = segmented_result_stream(
        stream::iter(row_iter).chain(stream::once(async move { trailer })),
        "query.http.stream",
        "http",
    );

    Response::builder()
        .status(200)
        .header(CONTENT_TYPE, "application/x-ndjson")
        .body(Body::from_stream(body_stream))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

/// `GET /api/v1/query/stream?sql=&stream=&stream_type=&from=&to=` ——
/// 浏览器 EventSource / fetch 可直接消费的 NDJSON 端点。
#[derive(Debug, Deserialize)]
pub struct StreamQuery {
    pub sql: String,
    pub stream: String,
    #[serde(default = "default_stream_type")]
    pub stream_type: crate::domain::stream::StreamType,
    /// epoch micros
    pub from: i64,
    /// epoch micros
    pub to: i64,
    pub limit: Option<usize>,
}

fn default_stream_type() -> crate::domain::stream::StreamType {
    crate::domain::stream::StreamType::Logs
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn stream_query_get(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(q): Query<StreamQuery>,
) -> Result<Response> {
    if q.to < q.from {
        return Err(Error::invalid("`to` must be >= `from`"));
    }
    let req = QueryRequest {
        org_id: ctx.org_id.clone(),
        language: QueryLanguage::Sql,
        statement: q.sql,
        time_range: TimeRange::new(TimestampMicros(q.from), TimestampMicros(q.to)),
        stream: Some(StreamHint {
            name: q.stream,
            stream_type: q.stream_type,
        }),
        limit: q.limit,
        federation_clusters: Vec::new(),
    };
    Ok(ndjson_response(
        state
            .query
            .run_tracked(req, ctx.user_id.clone(), ctx.organization_role_key())
            .await?,
    ))
}

/// `POST /api/v1/query/stream`：与 `/query` POST 同样的 body schema，强制 NDJSON 输出。
/// 客户端拿到 chunked response，便于 LogStream live tail（spec LogStream）。
#[permission(any("streams.query", "sys.telemetry.read"))]
async fn stream_query_post(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(mut req): Json<QueryRequest>,
) -> Result<Response> {
    req.org_id = ctx.org_id.clone();
    Ok(ndjson_response(
        state
            .query
            .run_tracked(req, ctx.user_id.clone(), ctx.organization_role_key())
            .await?,
    ))
}

/// `GET /api/v1/query/running`：列出活动查询。
/// 仅列出当前 IAM 组织上下文中的活动查询。
#[permission("org.settings.read")]
async fn list_running(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<ActiveQuerySnapshot>>> {
    let registry = state.query.registry();
    let scoped = registry.list_for(Some(&ctx.org_id));
    Ok(Json(scoped))
}

/// `POST /api/v1/query/{id}/cancel`：翻 cancel 标志。
/// 调用方必须位于该 query 所在的 IAM 组织上下文。
#[permission("org.settings.manage")]
async fn cancel_query(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let registry = state.query.registry();
    let owner_org = registry
        .lookup_org(&id)
        .ok_or_else(|| Error::not_found("query not found"))?;
    if owner_org != ctx.org_id {
        return Err(Error::not_found("query not found"));
    }
    registry.cancel(&id)?;
    // 联邦查询：除本地丢弃 future（gRPC 流断开兜底）外，向**实际参与**的远端定向 fan-out
    // 显式 CancelQuery（coordinator 记录的派发集群，不广播全网）。后台 fire-and-forget。
    if let Some(fed_id) = registry.federation_query_id(&id)
        && let Some(cluster_ids) = state.cluster.federation_cancel.dispatched(&fed_id)
        && !cluster_ids.is_empty()
    {
        let remotes = state.cluster.remote_clusters.clone();
        let org_links = state.cluster.org_link.clone();
        let secrets = state.cluster.secrets.clone();
        crate::shared::trace_context::spawn_with_current_trace_context(async move {
            fan_out_cancel(&fed_id, &cluster_ids, remotes, org_links, secrets).await
        });
    }
    Ok(Json(serde_json::json!({"cancelled": true})))
}

/// 向**参与该查询的**远端集群发 `CancelQuery(fed_id)`（best-effort）。token 经 per-org link
/// 解析（cluster 控制 RPC 用任一有 org 上下文的 token 即可），逐个软降级（连不上/无 token 跳过）。
async fn fan_out_cancel(
    fed_id: &str,
    cluster_ids: &[String],
    remotes: std::sync::Arc<dyn crate::infra::cluster::RemoteClustersRepository>,
    org_links: std::sync::Arc<
        dyn crate::infra::persistence::repositories::cluster::events::ClusterOrgLinkRepository,
    >,
    secrets: std::sync::Arc<dyn crate::infra::cluster::ClusterSecretRepository>,
) {
    use crate::{
        infra::{cluster::grpc_channel, secret::resolve_cluster_control_token},
        protocol::cluster::v1::{CancelQueryRequest, event_service_client::EventServiceClient},
    };

    for cid in cluster_ids {
        let Ok(c) = remotes.get(&Id(cid.clone())).await else {
            continue;
        };
        let per_org: Vec<(Id, Option<String>)> = org_links
            .list(&c.id)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|l| (l.local_org_id, l.token_secret_ref))
            .collect();
        let Some(token) =
            resolve_cluster_control_token(&c.token_secret_ref, &per_org, Some(secrets.as_ref()))
                .await
        else {
            continue;
        };
        let Ok(channel) = grpc_channel::connect(&c.advertise_addr, c.tls_verify).await else {
            continue;
        };
        let mut req = tonic::Request::new(CancelQueryRequest {
            federation_query_id: fed_id.to_string(),
        });
        if grpc_channel::with_bearer(&mut req, &token).is_err() {
            continue;
        }
        let mut client = EventServiceClient::new(channel);
        let _ = crate::shared::grpc_trace::call(
            req,
            "cluster.v1.EventService",
            "CancelQuery",
            crate::shared::grpc_trace::GrpcTarget::Internal,
            |request| client.cancel_query(request),
        )
        .await;
    }
}

#[derive(Debug, Deserialize)]
pub struct SearchAroundReq {
    /// 命中事件的指针：`(_timestamp_us, fingerprint)`；后端用它拉前后 N 条。
    pub event_timestamp_us: i64,
    pub event_fingerprint: Option<String>,
    pub stream: String,
    pub stream_type: crate::domain::stream::StreamType,
    #[serde(default = "default_around")]
    pub before: u32,
    #[serde(default = "default_around")]
    pub after: u32,
}

fn default_around() -> u32 {
    50
}

/// 基于 event pointer 前后各 N 条。
#[permission(any("streams.query", "sys.telemetry.read"))]
async fn search_around(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<SearchAroundReq>,
) -> Result<Json<Value>> {
    let before = req.before.clamp(1, 1000) as usize;
    let after = req.after.clamp(1, 1000) as usize;
    let ts = req.event_timestamp_us;

    // stream 不存在时返 200 + 空 before/after，避免 planner 把 NotFound 翻成 403。
    // search_around 的语义是"以指针为中心列出前后"，找不到 stream 等价于"无前后事件"。
    let stream_exists = state
        .telemetry
        .streams
        .get(&ctx.org_id, &req.stream, req.stream_type)
        .await
        .is_ok();
    if !stream_exists {
        let empty = QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            scanned_rows: 0,
            took_ms: 0,
            federation: None,
        };
        return Ok(Json(serde_json::json!({
            "pointer": {
                "ts": ts,
                "fingerprint": req.event_fingerprint,
            },
            "before": empty,
            "after": empty,
        })));
    }

    // before：order by _timestamp DESC limit before，where _timestamp <= ts
    let mk_req = |sql: String, range: TimeRange| QueryRequest {
        org_id: ctx.org_id.clone(),
        language: crate::domain::query::QueryLanguage::Sql,
        statement: sql,
        time_range: range,
        stream: Some(crate::domain::query::StreamHint {
            name: req.stream.clone(),
            stream_type: req.stream_type,
        }),
        limit: None,
        federation_clusters: Vec::new(),
    };

    let before_sql = format!(
        "SELECT * FROM {} WHERE _timestamp <= {} ORDER BY _timestamp DESC LIMIT {}",
        req.stream, ts, before
    );
    let after_sql = format!(
        "SELECT * FROM {} WHERE _timestamp > {} ORDER BY _timestamp ASC LIMIT {}",
        req.stream, ts, after
    );
    let wide = TimeRange::new(
        TimestampMicros(ts.saturating_sub(24 * 3600 * 1_000_000)),
        TimestampMicros(ts.saturating_add(24 * 3600 * 1_000_000)),
    );

    let before_out = state.query.run(mk_req(before_sql, wide)).await?;
    let after_out = state.query.run(mk_req(after_sql, wide)).await?;

    Ok(Json(serde_json::json!({
        "pointer": {
            "ts": ts,
            "fingerprint": req.event_fingerprint,
        },
        "before": before_out,
        "after": after_out,
    })))
}

#[cfg(test)]
mod tests {
    use super::parse_clusters_csv;

    #[test]
    fn parses_csv_trimming_and_dropping_empties() {
        assert_eq!(parse_clusters_csv(None), Vec::<String>::new());
        assert_eq!(parse_clusters_csv(Some("")), Vec::<String>::new());
        assert_eq!(parse_clusters_csv(Some("local")), vec!["local"]);
        assert_eq!(
            parse_clusters_csv(Some(" local , sf ,, nyc ")),
            vec!["local", "sf", "nyc"]
        );
    }
}
