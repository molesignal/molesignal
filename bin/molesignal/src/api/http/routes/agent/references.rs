// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 输入框里的 `@` 引用：搜索，以及发消息时的解析。
//!
//! - `GET /api/v1/agent/references?q=&kind=`：按名字找提问人能读的事件、告警规则、服务、仪表盘
//!   与数据流，每类最多 [`PER_KIND`] 个。
//! - [`resolve`]：发消息时逐个核对提问人能不能读，写成交给模型的上下文块；读不了的引用让这条
//!   消息被拒绝，不悄悄丢掉。

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    agent::references::{
        MAX_REFERENCES, MessageReference, REFERENCES_INTRODUCTION, ReferenceKind, context_block,
    },
    api::{
        AppState,
        http::middleware::{Permission, authorize_resource},
        rca::incident_facts,
    },
    app::iam::IamContext,
    domain::{
        alerting::{incident::Incident, rule::AlertRule},
        dashboard::Dashboard,
        iam::permission,
        stream::StreamDefinition,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 搜索时每类最多给几个。
const PER_KIND: usize = 8;
/// 服务从最近这段时间的调用关系里找。
const SERVICE_WINDOW_MICROS: i64 = 24 * 60 * 60 * 1_000_000;
/// 一个引用交给模型的内容最多这么多字符。
const FACTS_CHARS: usize = 4_000;

pub fn routes() -> Router<AppState> {
    Router::new().route("/agent/references", get(search))
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    kind: Option<ReferenceKind>,
}

fn matches(text: &str, query: &str) -> bool {
    query.is_empty() || text.to_lowercase().contains(query)
}

async fn allowed(
    state: &AppState,
    ctx: &IamContext,
    permission: &str,
    kind: &str,
    id: &str,
) -> bool {
    Permission::require_resource(state, ctx, permission, &ctx.org_id, kind, id)
        .await
        .is_ok()
}

fn result(kind: ReferenceKind, id: &str, label: &str, detail: String) -> Value {
    json!({ "kind": kind, "id": id, "label": label, "detail": detail })
}

#[permission("agent.use")]
async fn search(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Value>> {
    let q = query.q.trim().to_lowercase();
    let wants = |kind: ReferenceKind| query.kind.is_none_or(|want| want == kind);
    let mut results = Vec::new();

    if wants(ReferenceKind::Incident) && ctx.has_permission("alerts.read") {
        let since = TimestampMicros(TimestampMicros::now().0 - 7 * SERVICE_WINDOW_MICROS);
        let mut incidents = state
            .alerting
            .service
            .incidents
            .list_since(&ctx.org_id, since)
            .await?;
        incidents.sort_by_key(|incident| std::cmp::Reverse(incident.created_at.0));

        for incident in incidents
            .iter()
            .filter(|incident| matches(&incident.summary, &q))
        {
            if results.len() >= PER_KIND {
                break;
            }

            if allowed(&state, &ctx, "alerts.read", "alert", &incident.id.0).await {
                let detail = format!("{:?} · {:?}", incident.severity, incident.status);
                results.push(result(
                    ReferenceKind::Incident,
                    &incident.id.0,
                    &incident.summary,
                    detail,
                ));
            }
        }
    }

    let mut found = results.len();

    if wants(ReferenceKind::AlertRule) && ctx.has_permission("alerts.read") {
        let rules = state.alerting.service.list_rules(&ctx.org_id).await?;

        for rule in rules.iter().filter(|rule| matches(&rule.name, &q)) {
            if results.len() - found >= PER_KIND {
                break;
            }

            if allowed(&state, &ctx, "alerts.read", "alert", &rule.id.0).await {
                let detail = if rule.enabled { "enabled" } else { "disabled" }.to_string();
                results.push(result(
                    ReferenceKind::AlertRule,
                    &rule.id.0,
                    &rule.name,
                    detail,
                ));
            }
        }
    }

    found = results.len();

    if wants(ReferenceKind::Dashboard) && ctx.has_permission("dashboards.read") {
        let dashboards = state.dashboard.list(&ctx.org_id, None).await?;

        for dashboard in dashboards
            .iter()
            .filter(|dashboard| matches(&dashboard.title, &q))
        {
            if results.len() - found >= PER_KIND {
                break;
            }

            if allowed(
                &state,
                &ctx,
                "dashboards.read",
                "dashboard",
                &dashboard.id.0,
            )
            .await
            {
                let detail = dashboard.tags.join(", ");
                results.push(result(
                    ReferenceKind::Dashboard,
                    &dashboard.id.0,
                    &dashboard.title,
                    detail,
                ));
            }
        }
    }

    found = results.len();

    if wants(ReferenceKind::Stream) && ctx.has_permission("streams.read") {
        let streams = state.telemetry.streams.list(&ctx.org_id).await?;

        for stream in streams
            .iter()
            .filter(|stream| !stream.name.starts_with('_') && matches(&stream.name, &q))
        {
            if results.len() - found >= PER_KIND {
                break;
            }

            if allowed(&state, &ctx, "streams.read", "stream", &stream.id.0).await {
                let detail = stream.stream_type.to_string();
                results.push(result(
                    ReferenceKind::Stream,
                    &stream.id.0,
                    &stream.name,
                    detail,
                ));
            }
        }
    }

    if wants(ReferenceKind::Service) && ctx.has_permission("streams.query") {
        let mut names = service_names(&state, &ctx.org_id).await;
        names.retain(|name| matches(name, &q));
        names.truncate(PER_KIND);

        results.extend(
            names
                .iter()
                .map(|name| result(ReferenceKind::Service, name, name, String::new())),
        );
    }

    Ok(Json(json!({ "results": results })))
}

/// 最近一天调用关系里出现过的服务名，按名字排序。
async fn service_names(state: &AppState, org_id: &Id) -> Vec<String> {
    let now = TimestampMicros::now().0;

    let edges = state
        .telemetry
        .service_graph
        .query(org_id, now - SERVICE_WINDOW_MICROS, now, None)
        .await
        .unwrap_or_default();

    let mut names = edges
        .into_iter()
        .flat_map(|edge| [edge.client_service, edge.server_service])
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();

    names.sort();
    names.dedup();
    names
}

fn cut(text: String) -> String {
    if text.chars().count() <= FACTS_CHARS {
        return text;
    }

    let kept = text.chars().take(FACTS_CHARS).collect::<String>();
    format!("{kept}\n[cut]")
}

fn unavailable(reference: &MessageReference) -> Error {
    Error::invalid(format!(
        "the {} `{}` cannot be attached: it does not exist or you cannot read it",
        reference.kind.as_str(),
        reference.id
    ))
}

/// 核对并解析这条消息的引用：返回改写好名字的引用，与交给模型的上下文（没有引用时为空）。
pub(crate) async fn resolve(
    state: &AppState,
    ctx: &IamContext,
    references: Vec<MessageReference>,
) -> Result<(Vec<MessageReference>, Option<String>)> {
    if references.len() > MAX_REFERENCES {
        return Err(Error::invalid(format!(
            "a message carries at most {MAX_REFERENCES} references"
        )));
    }

    let mut resolved = Vec::new();
    let mut blocks = vec![REFERENCES_INTRODUCTION.to_string()];

    for mut reference in references {
        let id = Id::from_string(reference.id.trim());

        let (label, facts) = match reference.kind {
            ReferenceKind::Incident => {
                let incident: Incident = authorize_resource(state, ctx, id, "alerts.read")
                    .await
                    .map_err(|_| unavailable(&reference))?;

                (incident.summary.clone(), incident_facts(&incident))
            }
            ReferenceKind::AlertRule => {
                let rule: AlertRule = authorize_resource(state, ctx, id, "alerts.read")
                    .await
                    .map_err(|_| unavailable(&reference))?;

                (rule.name.clone(), rule_facts(&rule))
            }
            ReferenceKind::Dashboard => {
                let dashboard: Dashboard = authorize_resource(state, ctx, id, "dashboards.read")
                    .await
                    .map_err(|_| unavailable(&reference))?;

                (dashboard.title.clone(), dashboard_facts(&dashboard))
            }
            ReferenceKind::Stream => {
                let stream: StreamDefinition = authorize_resource(state, ctx, id, "streams.read")
                    .await
                    .map_err(|_| unavailable(&reference))?;

                let facts = format!(
                    "Stream: {}\nType: {}\nFields: {}",
                    stream.name,
                    stream.stream_type,
                    stream
                        .schema
                        .fields
                        .iter()
                        .take(80)
                        .map(|field| field.name.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                );

                (stream.name.clone(), facts)
            }
            ReferenceKind::Service => {
                if !ctx.has_permission("streams.query") {
                    return Err(unavailable(&reference));
                }

                let name = reference.id.trim().to_string();
                (name.clone(), service_facts(state, &ctx.org_id, &name).await)
            }
        };

        reference.label = label;
        blocks.push(context_block(&reference, &cut(facts)));
        resolved.push(reference);
    }

    let context = (!resolved.is_empty()).then(|| blocks.join("\n\n"));
    Ok((resolved, context))
}

fn rule_facts(rule: &AlertRule) -> String {
    format!(
        "Alert rule: {}\nDescription: {}\nEnabled: {}\nKind: {:?}\nSeverity: {:?}\nState: {:?}\nQuery: {}\nTrigger: {}\nLabels: {}",
        rule.name,
        rule.description,
        rule.enabled,
        rule.kind,
        rule.severity,
        rule.last_state,
        serde_json::to_string(&rule.query).unwrap_or_default(),
        serde_json::to_string(&rule.trigger).unwrap_or_default(),
        rule.labels
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn dashboard_facts(dashboard: &Dashboard) -> String {
    // 面板的标题与查询：模型据此知道这个仪表盘看的是什么。
    let panels = dashboard
        .model
        .get("panels")
        .and_then(Value::as_array)
        .map(|panels| {
            panels
                .iter()
                .take(30)
                .map(|panel| {
                    let title = panel
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("(untitled)");
                    let query = panel
                        .pointer("/targets/0/query")
                        .or_else(|| panel.get("query"))
                        .map(Value::to_string)
                        .unwrap_or_default();

                    format!("- {title} {query}")
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();

    format!(
        "Dashboard: {}\nTags: {}\nPanels:\n{}",
        dashboard.title,
        dashboard.tags.join(", "),
        panels
    )
}

async fn service_facts(state: &AppState, org_id: &Id, name: &str) -> String {
    let now = TimestampMicros::now().0;

    let edges = state
        .telemetry
        .service_graph
        .query(org_id, now - SERVICE_WINDOW_MICROS, now, Some(name))
        .await
        .unwrap_or_default();

    let mut lines = vec![format!("Service: {name}")];

    for edge in edges.iter().take(40) {
        lines.push(format!(
            "{} -> {}: {} requests, {} errors in the last day",
            edge.client_service, edge.server_service, edge.request_count, edge.error_count
        ));
    }

    lines.join("\n")
}
