// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 日志的自动发现。每个组织看最多 [`MAX_STREAMS`] 个带级别字段的日志流（最近改过结构的在前），
//! 只读错误级的日志：
//!
//! - **错误突增**（[`FindingKind::LogErrorSurge`]）：最近 15 分钟的条数，和前 1 小时每 15 分钟的
//!   平均比；流里有服务字段时按服务拆。
//! - **新错误**（[`FindingKind::LogNewError`]）：消息按模板归一（十六进制串、数字换成 `#`）后，最近
//!   15 分钟出现、此前一天没出现过的模板，每个流最多 [`MAX_NEW_TEMPLATES`] 条。
//!
//! 一个流查不出来时略过它，它进行中的发现这一轮不动（[`LogScan::scanned`]）。

use std::{collections::HashSet, sync::Arc};

use serde_json::{Value, json};

use crate::{
    domain::{
        alerting::{
            discovery::{Detection, FindingKind},
            incident::Severity,
        },
        query::{QueryEngine, QueryLanguage, QueryRequest, QueryResult, StreamHint},
        stream::{StreamDefinition, StreamRepository, StreamType},
    },
    shared::{
        Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

/// 每个组织最多看几个日志流。
pub const MAX_STREAMS: usize = 20;

/// 每个流最多报几条新错误。
pub const MAX_NEW_TEMPLATES: usize = 10;

const MINUTE: i64 = 60_000_000;

/// 最近这一段算「现在」。
const RECENT: i64 = 15 * MINUTE;

/// 拿来比的：再往前 1 小时。
const BEFORE: i64 = 60 * MINUTE;

/// 新错误往前看多久。
const NEW_LOOKBACK: i64 = 24 * 60 * MINUTE;

/// 突增至少这么多条才算，免得零星几条也报。
const MIN_SURGE: f64 = 20.0;

/// 新错误至少出现几次。
const MIN_NEW_HITS: f64 = 3.0;

/// 级别字段可能叫什么；数值的 `severity_number` 按 OpenTelemetry 的约定，17 起是错误。
const LEVEL_FIELDS: [&str; 6] = [
    "level",
    "severity",
    "severity_text",
    "log.level",
    "levelname",
    "loglevel",
];
const NUMBER_FIELD: &str = "severity_number";
const MESSAGE_FIELDS: [&str; 4] = ["message", "body", "msg", "log"];
const SERVICE_FIELDS: [&str; 4] = ["service.name", "service", "service_name", "app"];

/// 算错误的级别。
const ERROR_LEVELS: &str =
    "'error', 'err', 'fatal', 'critical', 'crit', 'alert', 'emerg', 'emergency', 'panic', 'severe'";

/// 一个日志流怎么读：哪个字段是级别、消息、服务。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogShape {
    pub stream: String,
    /// 错误级的条件。
    pub errors: String,
    pub message: Option<String>,
    pub service: Option<String>,
}

impl LogShape {
    /// 有级别字段的日志流才看。
    pub fn of(stream: &StreamDefinition) -> Option<Self> {
        if stream.stream_type != StreamType::LOGS || stream.name.starts_with('_') {
            return None;
        }

        let has = |name: &str| stream.schema.fields.iter().any(|field| field.name == name);
        let first = |names: &[&str]| {
            names
                .iter()
                .find(|name| has(name))
                .map(|name| name.to_string())
        };

        let errors = match first(&LEVEL_FIELDS) {
            Some(level) => format!(
                "lower(CAST({} AS VARCHAR)) IN ({ERROR_LEVELS})",
                quoted(&level)
            ),
            None if has(NUMBER_FIELD) => format!("{} >= 17", quoted(NUMBER_FIELD)),
            None => return None,
        };

        Some(Self {
            stream: stream.name.clone(),
            errors,
            message: first(&MESSAGE_FIELDS),
            service: first(&SERVICE_FIELDS),
        })
    }

    fn service_column(&self) -> String {
        match &self.service {
            Some(service) => format!("CAST({} AS VARCHAR)", quoted(service)),
            None => format!("'{}'", self.stream.replace('\'', "''")),
        }
    }

    /// 最近一段和前 1 小时的错误条数，按服务拆。
    pub fn surge_sql(&self, recent_from: i64) -> String {
        let service = self.service_column();

        format!(
            "SELECT {service} AS service, \
                    SUM(CASE WHEN CAST(_timestamp AS BIGINT) >= {recent_from} THEN 1 ELSE 0 END) AS recent, \
                    SUM(CASE WHEN CAST(_timestamp AS BIGINT) < {recent_from} THEN 1 ELSE 0 END) AS earlier \
             FROM {} WHERE {} GROUP BY {service}",
            quoted(&self.stream),
            self.errors
        )
    }

    /// 最近一段才第一次出现的错误模板，按次数排。
    pub fn new_errors_sql(&self, recent_from: i64) -> Option<String> {
        let message = self.message.as_ref()?;
        let service = self.service_column();
        let template = format!(
            "substr(regexp_replace(CAST({} AS VARCHAR), '[0-9a-fA-F]{{8,}}|[0-9]+', '#', 'g'), 1, 200)",
            quoted(message)
        );

        Some(format!(
            "SELECT {service} AS service, {template} AS template, \
                    MIN(CAST(_timestamp AS BIGINT)) AS first_seen, COUNT(*) AS hits \
             FROM {} WHERE {} GROUP BY {service}, {template} \
             HAVING MIN(CAST(_timestamp AS BIGINT)) >= {recent_from} \
             ORDER BY hits DESC LIMIT {MAX_NEW_TEMPLATES}",
            quoted(&self.stream),
            self.errors
        ))
    }
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// 一轮读出来的：找到的，和读到了的流。
#[derive(Debug, Default)]
pub struct LogScan {
    pub detections: Vec<Detection>,
    pub scanned: HashSet<String>,
}

pub struct LogDiscovery {
    engine: Arc<dyn QueryEngine>,
    streams: Arc<dyn StreamRepository>,
}

impl LogDiscovery {
    pub fn new(engine: Arc<dyn QueryEngine>, streams: Arc<dyn StreamRepository>) -> Self {
        Self { engine, streams }
    }

    pub async fn detect(&self, org_id: &Id, now: TimestampMicros) -> Result<LogScan> {
        let mut streams = self.streams.list(org_id).await?;

        streams.sort_by_key(|stream| std::cmp::Reverse(stream.updated_at.0));

        let shapes: Vec<LogShape> = streams
            .iter()
            .filter_map(LogShape::of)
            .take(MAX_STREAMS)
            .collect();
        let mut scan = LogScan::default();
        let recent_from = now.0 - RECENT;

        for shape in shapes {
            let surge = self
                .run(
                    org_id,
                    &shape,
                    shape.surge_sql(recent_from),
                    now.0 - RECENT - BEFORE,
                    now.0,
                )
                .await;

            let new_errors = match shape.new_errors_sql(recent_from) {
                Some(sql) => self
                    .run(org_id, &shape, sql, now.0 - NEW_LOOKBACK, now.0)
                    .await
                    .map(Some),
                None => Ok(None),
            };

            match (surge, new_errors) {
                (Ok(surge), Ok(new_errors)) => {
                    scan.detections.extend(surges(&shape, &surge, now));
                    scan.detections.extend(
                        new_errors
                            .map(|found| fresh_errors(&shape, &found))
                            .unwrap_or_default(),
                    );
                    scan.scanned.insert(shape.stream.clone());
                }
                (Err(error), _) | (_, Err(error)) => {
                    tracing::debug!(stream = %shape.stream, %error, "a log stream could not be read for discovery");
                }
            }
        }

        Ok(scan)
    }

    async fn run(
        &self,
        org_id: &Id,
        shape: &LogShape,
        statement: String,
        from: i64,
        to: i64,
    ) -> Result<QueryResult> {
        self.engine
            .execute(QueryRequest {
                org_id: org_id.clone(),
                language: QueryLanguage::Sql,
                statement,
                time_range: TimeRange::new(TimestampMicros(from), TimestampMicros(to)),
                stream: Some(StreamHint {
                    name: shape.stream.clone(),
                    stream_type: StreamType::LOGS,
                }),
                limit: None,
                step: None,
                federation_clusters: Vec::new(),
            })
            .await
    }
}

fn column(result: &QueryResult, name: &str) -> Option<usize> {
    result.columns.iter().position(|column| column == name)
}

fn number(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(number)) => number.as_f64().unwrap_or_default(),
        Some(Value::String(text)) => text.parse().unwrap_or_default(),
        _ => 0.0,
    }
}

fn text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// 最近 15 分钟比前 1 小时平均每 15 分钟多出很多的服务。
pub fn surges(shape: &LogShape, result: &QueryResult, now: TimestampMicros) -> Vec<Detection> {
    let (Some(service), Some(recent), Some(earlier)) = (
        column(result, "service"),
        column(result, "recent"),
        column(result, "earlier"),
    ) else {
        return Vec::new();
    };

    result
        .rows
        .iter()
        .filter_map(|row| {
            let subject = text(row.get(service));
            let now_count = number(row.get(recent));
            let usual = number(row.get(earlier)) / (BEFORE / RECENT) as f64;
            let ratio = now_count / usual.max(1.0);

            if now_count < MIN_SURGE || ratio < 3.0 {
                return None;
            }

            Some(Detection {
                kind: FindingKind::LogErrorSurge,
                key: format!("log_error_surge|{}|{subject}", shape.stream),
                severity: if ratio >= 10.0 { Severity::Error } else { Severity::Warning },
                subject: subject.clone(),
                environment: None,
                metric: Some("error_logs".into()),
                title: format!(
                    "Error logs of {subject} in {} are up {ratio:.0}×: {now_count:.0} in 15 minutes",
                    shape.stream
                ),
                value: Some(now_count),
                baseline: Some(usual),
                started_at: Some(TimestampMicros(now.0 - RECENT)),
                detail: json!({ "stream": shape.stream, "service": shape.service.as_ref().map(|_| subject) }),
            })
        })
        .collect()
}

/// 最近一段才出现的错误模板。
pub fn fresh_errors(shape: &LogShape, result: &QueryResult) -> Vec<Detection> {
    let (Some(service), Some(template), Some(first_seen), Some(hits)) = (
        column(result, "service"),
        column(result, "template"),
        column(result, "first_seen"),
        column(result, "hits"),
    ) else {
        return Vec::new();
    };

    result
        .rows
        .iter()
        .filter_map(|row| {
            let subject = text(row.get(service));
            let template = text(row.get(template));
            let count = number(row.get(hits));

            if template.trim().is_empty() || count < MIN_NEW_HITS {
                return None;
            }

            let digest = blake3::hash(template.as_bytes()).to_hex();

            Some(Detection {
                kind: FindingKind::LogNewError,
                key: format!("log_new_error|{}|{subject}|{}", shape.stream, &digest[..16]),
                severity: if count >= 50.0 {
                    Severity::Error
                } else {
                    Severity::Warning
                },
                subject: subject.clone(),
                environment: None,
                metric: None,
                title: format!(
                    "New error in {subject}: {}",
                    template.chars().take(120).collect::<String>()
                ),
                value: Some(count),
                baseline: None,
                started_at: Some(TimestampMicros(number(row.get(first_seen)) as i64)),
                detail: json!({ "stream": shape.stream, "template": template }),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
