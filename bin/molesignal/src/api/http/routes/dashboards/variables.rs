// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Dashboard variable 候选值解析端点：`POST /api/v1/dashboards/variables/resolve`。
//!
//! Dashboard 顶部的 variable dropdown 取候选值时调用此端点。前端把单
//! variable 的定义（`label_values(metric, label)` 或自由 SQL）+ 当前时间窗发过
//! 来，后端跑 distinct query 把候选返回，前端用首个值做默认。
//!
//! 解析规则：
//! - `label_values(<metric>, <label>)`：在指标目录里持有该指标、带这个标签的每个指标流上跑
//!   `SELECT DISTINCT <label> FROM <stream> WHERE metric_name = '<metric>'`，合并去重；body 可以
//!   显式指定只查一个流；
//! - 其他：原样作为 SQL 执行，必须由前端在 `stream` 字段里提供 hint。
//!
//! 不在后端扩展查询宏（例如 `$__rate_interval`）— 这些由前端处理。
//! variable substitution 时已经展开。

use axum::{Extension, Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::permission,
        metrics::METRIC_NAME_FIELD,
        query::{QueryLanguage, QueryRequest, StreamHint},
        stream::StreamType,
    },
    shared::{Error, Result, time::TimeRange},
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/dashboards/variables/resolve", post(resolve))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveRequest {
    pub variable: VariableSpec,
    pub time_range: TimeRangeInput,
    /// 限制返回 distinct 值数量，默认 100；防 IdP/前端 dropdown 爆炸。
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct VariableSpec {
    pub name: String,
    pub query: String,
    /// 'query'（默认，走 label_values 解析） | 'sql'（透传自由 SQL）
    #[serde(default = "default_kind")]
    pub kind: String,
    /// 自由 SQL 模式 / 显式覆盖 stream 时使用。默认走 label_values metric 推断。
    #[serde(default)]
    pub stream: Option<StreamHintInput>,
}

#[derive(Debug, Deserialize)]
pub struct TimeRangeInput {
    pub start: i64,
    pub end: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamHintInput {
    pub name: String,
    pub stream_type: StreamType,
}

#[derive(Debug, Serialize)]
pub struct ResolveResponse {
    pub variable: String,
    pub values: Vec<String>,
    pub default: Option<String>,
}

fn default_kind() -> String {
    "query".to_string()
}

#[permission(any("dashboards.read", "sys.dashboards.read"))]
async fn resolve(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<ResolveRequest>,
) -> Result<Json<ResolveResponse>> {
    let limit = req.limit.unwrap_or(100).min(500);
    let raw_query = req.variable.query.trim().to_string();

    if raw_query.is_empty() {
        return Err(Error::invalid("variable.query empty"));
    }

    let time_range = TimeRange::new(
        crate::shared::time::TimestampMicros(req.time_range.start),
        crate::shared::time::TimestampMicros(req.time_range.end),
    );

    let queries = match req.variable.kind.as_str() {
        "sql" => {
            let hint = req
                .variable
                .stream
                .as_ref()
                .ok_or_else(|| Error::invalid("kind = sql requires `stream` hint"))?;

            vec![(
                raw_query,
                StreamHint {
                    name: hint.name.clone(),
                    stream_type: hint.stream_type,
                },
            )]
        }
        "query" | "" => {
            // 默认按 label_values 解
            let (metric, label) = parse_label_values(&raw_query).ok_or_else(|| {
                Error::invalid(format!(
                    "variable.query `{raw_query}` not recognized; expected \
                     `label_values(<metric>, <label>)` or kind = sql"
                ))
            })?;

            let streams =
                label_streams(&state, &ctx, &req.variable, &metric, &label, time_range).await?;

            streams
                .into_iter()
                .map(|hint| {
                    let sql = format!(
                        "SELECT DISTINCT {label} FROM {stream} \
                         WHERE {name} = '{metric}' AND {label} IS NOT NULL AND {label} != '' \
                         ORDER BY {label} \
                         LIMIT {limit}",
                        label = quote_ident(&label),
                        stream = quote_ident(&hint.name),
                        name = quote_ident(METRIC_NAME_FIELD),
                    );

                    (sql, hint)
                })
                .collect()
        }
        other => {
            return Err(Error::invalid(format!(
                "variable.kind = '{other}'; expected 'query' or 'sql'"
            )));
        }
    };

    let mut values: Vec<String> = Vec::new();

    for (statement, hint) in queries {
        let result = state
            .query
            .run(QueryRequest {
                org_id: ctx.org_id.clone(),
                language: QueryLanguage::Sql,
                statement,
                time_range,
                stream: Some(hint),
                limit: Some(limit),
                step: None,
                federation_clusters: Vec::new(),
            })
            .await?;

        for row in &result.rows {
            if let Some(v) = row.first()
                && let Some(s) = json_as_string(v)
                && !s.is_empty()
                && !values.contains(&s)
            {
                values.push(s);
            }
        }
    }

    values.sort();
    values.truncate(limit);

    let default = values.first().cloned();

    Ok(Json(ResolveResponse {
        variable: req.variable.name,
        values,
        default,
    }))
}

fn parse_label_values(input: &str) -> Option<(String, String)> {
    let s = input.trim();
    let inner = s.strip_prefix("label_values")?.trim();
    let inner = inner.strip_prefix('(')?;
    let inner = inner.strip_suffix(')')?;
    let mut parts = inner.splitn(2, ',');
    let metric = parts.next()?.trim();
    let label = parts.next()?.trim();

    if metric.is_empty() || label.is_empty() {
        return None;
    }

    let metric = metric.trim_matches('"').trim_matches('\'');
    let label = label.trim_matches('"').trim_matches('\'');

    if !valid_ident(metric) || !valid_ident(label) {
        return None;
    }

    Some((metric.to_string(), label.to_string()))
}

fn valid_ident(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':' || c == '-' || c == '.')
        && s.chars()
            .next()
            .map(|c| c.is_ascii_alphabetic() || c == '_')
            .unwrap_or(false)
}

fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// The streams `label_values(metric, label)` reads: the one the variable names, or every queryable
/// metrics stream the directory says held the metric in `time_range` and whose schema has `label`.
async fn label_streams(
    state: &AppState,
    ctx: &IamContext,
    variable: &VariableSpec,
    metric: &str,
    label: &str,
    time_range: TimeRange,
) -> Result<Vec<StreamHint>> {
    if let Some(stream) = &variable.stream {
        return Ok(vec![StreamHint {
            name: stream.name.clone(),
            stream_type: stream.stream_type,
        }]);
    }

    let entries = state
        .telemetry
        .metric_directory
        .find(&ctx.org_id, metric)
        .await?;

    let mut streams = Vec::new();

    for entry in entries.iter().filter(|entry| entry.overlaps(time_range)) {
        let definition = match state
            .telemetry
            .streams
            .get(&ctx.org_id, &entry.stream_name, StreamType::METRICS)
            .await
        {
            Ok(definition) => definition,
            Err(Error::NotFound(_)) => continue,
            Err(error) => return Err(error),
        };

        let labelled = definition
            .schema
            .fields
            .iter()
            .any(|field| field.name == label);

        if labelled
            && state
                .telemetry
                .streams
                .get_settings(&definition.id)
                .await?
                .queryable
        {
            streams.push(StreamHint {
                name: definition.name,
                stream_type: StreamType::METRICS,
            });
        }
    }

    Ok(streams)
}

fn json_as_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => None,
        _ => Some(v.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_label_values_basic() {
        let (m, l) = parse_label_values("label_values(http_requests_total, service)").unwrap();
        assert_eq!(m, "http_requests_total");
        assert_eq!(l, "service");
    }

    #[test]
    fn parses_label_values_with_quotes() {
        let (m, l) = parse_label_values("label_values(\"app_logs\", \"level\")").unwrap();
        assert_eq!(m, "app_logs");
        assert_eq!(l, "level");
    }

    #[test]
    fn rejects_invalid_input() {
        assert!(parse_label_values("rate(http_requests_total[5m])").is_none());
        assert!(parse_label_values("label_values()").is_none());
        assert!(parse_label_values("label_values(metric)").is_none());
        // SQL injection 字段名一律拒绝
        assert!(parse_label_values("label_values(metric, label; DROP TABLE x)").is_none());
    }
}
