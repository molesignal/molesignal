// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 公共 OTLP 接入、进程内 finished-span exporter、tail sampler 与两个 sink
//! 共用的有界 Trace 契约。
//!
//! MoleSignal 固定使用 OpenTelemetry Semantic Conventions 1.43.0。标准字段保持
//! 规范名称；产品私有字段必须位于 `molesignal.*` 命名空间。

use std::{
    collections::{BTreeMap, HashSet},
    sync::OnceLock,
};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::summary::{
    TRACE_SUMMARY_DURATION_NS_FIELD, TRACE_SUMMARY_ERROR_COUNT_FIELD, TRACE_SUMMARY_MARKER_FIELD,
    TRACE_SUMMARY_SPAN_COUNT_FIELD, TRACE_SUMMARY_START_NS_FIELD,
};
use crate::{
    domain::{
        ingestion::RawEvent,
        stream::{FieldDef, FieldType, Schema},
    },
    shared::{Error, Result, time::TimestampMicros},
};

pub const SEMCONV_VERSION: &str = "1.43.0";
pub const CANONICAL_SPAN_SCHEMA_VERSION: u16 = 1;
const TRACE_FINGERPRINT_KEY_ENV: &str = "MS_TRACE_FINGERPRINT_KEY";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SamplingReason {
    #[default]
    Pending,
    TrustedForced,
    DebugForced,
    Error,
    Slow,
    Rule,
    Ratio,
    PressureRatio,
    PressureDrop,
    Disabled,
    NoOwner,
}

impl SamplingReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::TrustedForced => "trusted_forced",
            Self::DebugForced => "debug_forced",
            Self::Error => "error",
            Self::Slow => "slow",
            Self::Rule => "rule",
            Self::Ratio => "ratio",
            Self::PressureRatio => "pressure_ratio",
            Self::PressureDrop => "pressure_drop",
            Self::Disabled => "disabled",
            Self::NoOwner => "no_owner",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartialReason {
    AttributeLimit,
    EventLimit,
    LinkLimit,
    StringLimit,
    SpanLimit,
    SamplerOverflow,
    LateSpan,
    OwnerFailure,
    SinkDrop,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CanonicalResource {
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
    #[serde(default)]
    pub dropped_attributes_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CanonicalScope {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
    #[serde(default)]
    pub dropped_attributes_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalEvent {
    pub time_unix_nano: u64,
    pub name: String,
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
    #[serde(default)]
    pub dropped_attributes_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalLink {
    pub trace_id: String,
    pub span_id: String,
    #[serde(default)]
    pub trace_state: String,
    #[serde(default)]
    pub flags: u32,
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
    #[serde(default)]
    pub dropped_attributes_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalSpan {
    pub trace_id: String,
    pub span_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    #[serde(default)]
    pub trace_flags: u32,
    #[serde(default)]
    pub trace_state: String,
    pub name: String,
    pub kind: i32,
    pub start_time_unix_nano: u64,
    pub end_time_unix_nano: u64,
    pub duration_ns: u64,
    pub status_code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
    #[serde(default)]
    pub resource: CanonicalResource,
    #[serde(default)]
    pub scope: CanonicalScope,
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
    #[serde(default)]
    pub events: Vec<CanonicalEvent>,
    #[serde(default)]
    pub links: Vec<CanonicalLink>,
    #[serde(default)]
    pub dropped_attributes_count: u32,
    #[serde(default)]
    pub dropped_events_count: u32,
    #[serde(default)]
    pub dropped_links_count: u32,
    #[serde(default = "canonical_schema_version")]
    pub schema_version: u16,
    #[serde(default = "semconv_version")]
    pub semantic_conventions_version: String,
    #[serde(default)]
    pub sampling_reason: SamplingReason,
    #[serde(default)]
    pub partial: bool,
    #[serde(default)]
    pub partial_reasons: Vec<PartialReason>,
    #[serde(default)]
    pub late: bool,
    #[serde(default)]
    pub duplicate: bool,
    #[serde(default)]
    pub conflict: bool,
}

fn canonical_schema_version() -> u16 {
    CANONICAL_SPAN_SCHEMA_VERSION
}

fn semconv_version() -> String {
    SEMCONV_VERSION.to_string()
}

impl CanonicalSpan {
    pub fn new(
        trace_id: String,
        span_id: String,
        name: String,
        kind: i32,
        start_time_unix_nano: u64,
        end_time_unix_nano: u64,
    ) -> Self {
        Self {
            trace_id,
            span_id,
            parent_span_id: None,
            trace_flags: 0,
            trace_state: String::new(),
            name,
            kind,
            start_time_unix_nano,
            end_time_unix_nano,
            duration_ns: end_time_unix_nano.saturating_sub(start_time_unix_nano),
            status_code: "UNSET".into(),
            status_message: None,
            resource: CanonicalResource::default(),
            scope: CanonicalScope::default(),
            attributes: BTreeMap::new(),
            events: Vec::new(),
            links: Vec::new(),
            dropped_attributes_count: 0,
            dropped_events_count: 0,
            dropped_links_count: 0,
            schema_version: CANONICAL_SPAN_SCHEMA_VERSION,
            semantic_conventions_version: SEMCONV_VERSION.into(),
            sampling_reason: SamplingReason::Pending,
            partial: false,
            partial_reasons: Vec::new(),
            late: false,
            duplicate: false,
            conflict: false,
        }
    }

    pub fn mark_partial(&mut self, reason: PartialReason) {
        self.partial = true;
        if !self.partial_reasons.contains(&reason) {
            self.partial_reasons.push(reason);
        }
    }

    /// 稳定内容摘要，用于 `(org_id, trace_id, span_id)` 的相同重试/冲突判定。
    pub fn content_digest(&self) -> [u8; 32] {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        *blake3::hash(&bytes).as_bytes()
    }

    /// 只读取 [`Self::into_raw_event`] 写出的稳定 canonical 字段。用于启动早期
    /// tracing callback queue 在 AppState/TracePipeline 激活后无损接管。
    pub fn try_from_raw_event(event: &RawEvent) -> Result<Self> {
        let fields = &event.fields;
        let required_string = |name: &str| -> Result<String> {
            fields
                .get(name)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| Error::invalid(format!("canonical Trace row missing `{name}`")))
        };
        let required_u64 = |name: &str| -> Result<u64> {
            fields
                .get(name)
                .and_then(Value::as_u64)
                .ok_or_else(|| Error::invalid(format!("canonical Trace row missing `{name}`")))
        };
        let deserialize = |name: &str| -> Result<Value> {
            fields
                .get(name)
                .cloned()
                .ok_or_else(|| Error::invalid(format!("canonical Trace row missing `{name}`")))
        };
        Ok(Self {
            trace_id: required_string("trace_id")?,
            span_id: required_string("span_id")?,
            parent_span_id: fields
                .get("parent_span_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            trace_flags: fields
                .get("trace_flags")
                .and_then(Value::as_u64)
                .unwrap_or_default() as u32,
            trace_state: fields
                .get("trace_state")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            name: required_string("name")?,
            kind: fields
                .get("kind")
                .and_then(Value::as_i64)
                .unwrap_or_default() as i32,
            start_time_unix_nano: required_u64("start_time_unix_nano")?,
            end_time_unix_nano: required_u64("end_time_unix_nano")?,
            duration_ns: required_u64("duration_ns")?,
            status_code: required_string("status_code")?,
            status_message: fields
                .get("status_message")
                .and_then(Value::as_str)
                .map(str::to_owned),
            resource: serde_json::from_value(deserialize("resource")?)
                .map_err(|error| Error::invalid(format!("canonical resource: {error}")))?,
            scope: serde_json::from_value(deserialize("scope")?)
                .map_err(|error| Error::invalid(format!("canonical scope: {error}")))?,
            attributes: serde_json::from_value(deserialize("attributes")?)
                .map_err(|error| Error::invalid(format!("canonical attributes: {error}")))?,
            events: serde_json::from_value(deserialize("events")?)
                .map_err(|error| Error::invalid(format!("canonical events: {error}")))?,
            links: serde_json::from_value(deserialize("links")?)
                .map_err(|error| Error::invalid(format!("canonical links: {error}")))?,
            dropped_attributes_count: fields
                .get("dropped_attributes_count")
                .and_then(Value::as_u64)
                .unwrap_or_default() as u32,
            dropped_events_count: fields
                .get("dropped_events_count")
                .and_then(Value::as_u64)
                .unwrap_or_default() as u32,
            dropped_links_count: fields
                .get("dropped_links_count")
                .and_then(Value::as_u64)
                .unwrap_or_default() as u32,
            schema_version: fields
                .get("schema_version")
                .and_then(Value::as_u64)
                .unwrap_or(CANONICAL_SPAN_SCHEMA_VERSION as u64) as u16,
            semantic_conventions_version: fields
                .get("semantic_conventions_version")
                .and_then(Value::as_str)
                .unwrap_or(SEMCONV_VERSION)
                .to_owned(),
            sampling_reason: fields
                .get("sampling_reason")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok())
                .unwrap_or_default(),
            partial: fields
                .get("partial")
                .and_then(Value::as_bool)
                .unwrap_or_default(),
            partial_reasons: fields
                .get("partial_reasons")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok())
                .unwrap_or_default(),
            late: fields
                .get("late")
                .and_then(Value::as_bool)
                .unwrap_or_default(),
            duplicate: fields
                .get("duplicate")
                .and_then(Value::as_bool)
                .unwrap_or_default(),
            conflict: fields
                .get("conflict")
                .and_then(Value::as_bool)
                .unwrap_or_default(),
        })
    }

    /// 存储契约既保留完整嵌套结构，也复制常用标准维度为顶层列，兼容现有
    /// Trace 查询、Tantivy exact 索引和服务图。
    pub fn into_raw_event(self) -> RawEvent {
        let mut fields = Map::new();
        fields.insert("trace_id".into(), Value::String(self.trace_id));
        fields.insert("span_id".into(), Value::String(self.span_id));
        if let Some(parent_span_id) = self.parent_span_id {
            fields.insert("parent_span_id".into(), Value::String(parent_span_id));
        }
        fields.insert("trace_flags".into(), Value::from(self.trace_flags));
        fields.insert("trace_state".into(), Value::String(self.trace_state));
        fields.insert("name".into(), Value::String(self.name));
        fields.insert("kind".into(), Value::from(self.kind));
        fields.insert(
            "start_time_unix_nano".into(),
            Value::from(self.start_time_unix_nano),
        );
        fields.insert(
            "end_time_unix_nano".into(),
            Value::from(self.end_time_unix_nano),
        );
        fields.insert("duration_ns".into(), Value::from(self.duration_ns));
        fields.insert("status_code".into(), Value::String(self.status_code));
        if let Some(status_message) = self.status_message {
            fields.insert("status_message".into(), Value::String(status_message));
        }

        for (key, value) in &self.resource.attributes {
            fields.insert(key.clone(), value.clone());
        }
        for (key, value) in &self.attributes {
            fields.insert(key.clone(), value.clone());
        }
        fields
            .entry("service.name")
            .or_insert_with(|| Value::String("unknown_service".into()));
        let effective_service = effective_service_name(
            fields.get("service.name").and_then(Value::as_str),
            fields.get("service.namespace").and_then(Value::as_str),
            fields
                .get("molesignal.execution.role")
                .and_then(Value::as_str),
        );
        fields.insert("service.name".into(), Value::String(effective_service));

        fields.insert(
            "resource".into(),
            serde_json::to_value(&self.resource).unwrap_or(Value::Null),
        );
        fields.insert(
            "scope".into(),
            serde_json::to_value(&self.scope).unwrap_or(Value::Null),
        );
        fields.insert(
            "attributes".into(),
            serde_json::to_value(&self.attributes).unwrap_or(Value::Null),
        );
        fields.insert(
            "events".into(),
            serde_json::to_value(&self.events).unwrap_or(Value::Null),
        );
        fields.insert(
            "links".into(),
            serde_json::to_value(&self.links).unwrap_or(Value::Null),
        );
        fields.insert(
            "dropped_attributes_count".into(),
            Value::from(self.dropped_attributes_count),
        );
        fields.insert(
            "dropped_events_count".into(),
            Value::from(self.dropped_events_count),
        );
        fields.insert(
            "dropped_links_count".into(),
            Value::from(self.dropped_links_count),
        );
        fields.insert("schema_version".into(), Value::from(self.schema_version));
        fields.insert(
            "semantic_conventions_version".into(),
            Value::String(self.semantic_conventions_version),
        );
        fields.insert(
            "sampling_reason".into(),
            serde_json::to_value(self.sampling_reason).unwrap_or(Value::Null),
        );
        fields.insert("partial".into(), Value::Bool(self.partial));
        fields.insert(
            "partial_reasons".into(),
            serde_json::to_value(self.partial_reasons).unwrap_or(Value::Null),
        );
        fields.insert("late".into(), Value::Bool(self.late));
        fields.insert("duplicate".into(), Value::Bool(self.duplicate));
        fields.insert("conflict".into(), Value::Bool(self.conflict));

        RawEvent {
            timestamp: TimestampMicros((self.start_time_unix_nano / 1_000) as i64),
            fields,
        }
    }
}

/// Derive the query/service-graph identity for MoleSignal's multi-role process while keeping the
/// original process Resource intact in the nested canonical `resource` field.
pub fn effective_service_name(
    service: Option<&str>,
    namespace: Option<&str>,
    execution_role: Option<&str>,
) -> String {
    let role = execution_role
        .map(|role| role.trim().to_ascii_lowercase().replace('-', "_"))
        .filter(|role| {
            matches!(
                role.as_str(),
                "router" | "querier" | "ingester" | "compactor" | "alert_manager" | "standalone"
            )
        })
        .filter(|_| {
            namespace == Some("molesignal")
                || service
                    .is_some_and(|name| name == "molesignal" || name.starts_with("molesignal-"))
        });
    match role {
        Some(role) => format!("molesignal-{}", role.replace('_', "-")),
        None => service.unwrap_or("unknown_service").to_owned(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceLimits {
    pub max_attributes_per_span: usize,
    pub max_events_per_span: usize,
    pub max_links_per_span: usize,
    pub max_string_bytes: usize,
    pub max_spans_per_trace: usize,
}

impl Default for TraceLimits {
    fn default() -> Self {
        Self {
            max_attributes_per_span: 128,
            max_events_per_span: 128,
            max_links_per_span: 128,
            max_string_bytes: 4 * 1024,
            max_spans_per_trace: 1_000,
        }
    }
}

impl TraceLimits {
    pub fn validate(self) -> Result<Self> {
        if self.max_attributes_per_span == 0
            || self.max_events_per_span == 0
            || self.max_links_per_span == 0
            || self.max_string_bytes == 0
            || self.max_spans_per_trace == 0
        {
            return Err(Error::invalid("all Trace limits must be greater than zero"));
        }
        Ok(self)
    }
}

/// Fresh `_sys/traces/_molesignal` streams start with this stable field order.
/// There is intentionally no legacy-schema union or migration.
pub fn canonical_trace_schema() -> Schema {
    let field = |name: &str, data_type: FieldType, indexed: bool, exact: bool| FieldDef {
        name: name.into(),
        data_type,
        nullable: true,
        indexed,
        encrypted: false,
        exact,
    };
    Schema {
        fields: vec![
            field("trace_id", FieldType::Utf8, true, true),
            field("span_id", FieldType::Utf8, true, true),
            field("parent_span_id", FieldType::Utf8, false, false),
            field("trace_flags", FieldType::Int64, false, false),
            field("trace_state", FieldType::Utf8, false, false),
            field("name", FieldType::Utf8, false, false),
            field("kind", FieldType::Int64, false, false),
            field("start_time_unix_nano", FieldType::Int64, false, false),
            field("end_time_unix_nano", FieldType::Int64, false, false),
            field("duration_ns", FieldType::Int64, false, false),
            field("status_code", FieldType::Utf8, false, false),
            field("status_message", FieldType::Utf8, false, false),
            field("service.namespace", FieldType::Utf8, false, false),
            field("service.name", FieldType::Utf8, true, true),
            field("molesignal.execution.role", FieldType::Utf8, false, false),
            field(TRACE_SUMMARY_MARKER_FIELD, FieldType::Utf8, true, true),
            field(TRACE_SUMMARY_START_NS_FIELD, FieldType::Int64, false, false),
            field(
                TRACE_SUMMARY_DURATION_NS_FIELD,
                FieldType::Int64,
                false,
                false,
            ),
            field(
                TRACE_SUMMARY_SPAN_COUNT_FIELD,
                FieldType::Int64,
                false,
                false,
            ),
            field(
                TRACE_SUMMARY_ERROR_COUNT_FIELD,
                FieldType::Int64,
                false,
                false,
            ),
            field("resource", FieldType::Json, false, false),
            field("scope", FieldType::Json, false, false),
            field("attributes", FieldType::Json, false, false),
            field("events", FieldType::Json, false, false),
            field("links", FieldType::Json, false, false),
            field("dropped_attributes_count", FieldType::Int64, false, false),
            field("dropped_events_count", FieldType::Int64, false, false),
            field("dropped_links_count", FieldType::Int64, false, false),
            field("schema_version", FieldType::Int64, false, false),
            field(
                "semantic_conventions_version",
                FieldType::Utf8,
                false,
                false,
            ),
            field("sampling_reason", FieldType::Utf8, false, false),
            field("partial", FieldType::Bool, false, false),
            field("partial_reasons", FieldType::Json, false, false),
            field("late", FieldType::Bool, false, false),
            field("duplicate", FieldType::Bool, false, false),
            field("conflict", FieldType::Bool, false, false),
        ],
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SanitizeStats {
    pub removed: u32,
    pub redacted: u32,
    pub truncated: u32,
}

const PRIORITY_ATTRIBUTES: &[&str] = &[
    "error.type",
    "exception.type",
    "exception.message",
    "http.request.method",
    "http.response.status_code",
    "rpc.grpc.status_code",
    "db.operation.name",
    "db.collection.name",
    "service.name",
    "molesignal.execution.role",
];

fn forbidden_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase().replace(['-', '_'], ".");
    matches!(
        key.as_str(),
        "authorization"
            | "cookie"
            | "set.cookie"
            | "password"
            | "passwd"
            | "pwd"
            | "body"
            | "request.body"
            | "response.body"
            | "http.request.body"
            | "http.response.body"
            | "http.target"
            | "url.full"
            | "url.original"
            | "url.path"
            | "url.query"
            | "db.statement"
            | "db.query.text"
            | "db.query.parameter"
            | "object.key"
            | "aws.s3.key"
            | "file.path"
            | "file.name"
            | "email"
            | "user.email"
            | "user.name"
            | "enduser.name"
            | "first.name"
            | "last.name"
            | "display.name"
            | "username"
            | "recipient"
            | "notification.recipient"
            | "notification.subject"
            | "notification.content"
            | "gen.ai.prompt"
            | "gen.ai.completion"
            | "gen.ai.input"
            | "gen.ai.output"
            | "llm.prompt"
            | "llm.response"
            | "tool.arguments"
            | "tool.args"
            | "tool.result"
            | "tool.output"
            | "license.package"
            | "license.signature"
            | "license.signed.payload"
            | "private.key"
            | "secret.reference"
    ) || key.ends_with(".authorization")
        || key.ends_with(".cookie")
        || key.ends_with(".password")
        || key.ends_with(".secret")
        || key.ends_with(".credential")
        || key.ends_with(".token")
        || key.ends_with(".email")
        || key.ends_with(".recipient")
        || key.ends_with(".signature")
        || key.ends_with(".private.key")
        || key.contains(".request.header.")
        || key.contains(".response.header.")
        || key.contains(".request.metadata.")
        || key.contains("signature.b64")
        || key.contains("payload.b64")
}

fn truncate_utf8(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

/// 对完整对象 key、SQL shape 等敏感但需要稳定关联的值生成可选 keyed fingerprint。
/// key 只从进程环境读取并缓存，绝不写入 config diff、日志、Span 或响应。未配置时
/// 返回 None，调用方不得退回 unkeyed hash（后者容易被字典反推）。
pub fn optional_hmac_fingerprint(value: &str) -> Option<String> {
    static KEY: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    let key = KEY.get_or_init(|| {
        std::env::var(TRACE_FINGERPRINT_KEY_ENV)
            .ok()
            .map(|value| value.into_bytes())
            .filter(|value| value.len() >= 16)
    });
    key.as_deref()
        .map(|key| hex::encode(&hmac_sha256(key, value.as_bytes())[..16]))
}

fn hmac_sha256(key: &[u8], value: &[u8]) -> [u8; 32] {
    const BLOCK_BYTES: usize = 64;
    let mut normalized = [0_u8; BLOCK_BYTES];
    if key.len() > BLOCK_BYTES {
        normalized[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        normalized[..key.len()].copy_from_slice(key);
    }
    let mut inner_pad = [0x36_u8; BLOCK_BYTES];
    let mut outer_pad = [0x5c_u8; BLOCK_BYTES];
    for index in 0..BLOCK_BYTES {
        inner_pad[index] ^= normalized[index];
        outer_pad[index] ^= normalized[index];
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(value);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner);
    outer.finalize().into()
}

fn sensitive_value_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"(?i)\b(?:bearer|basic)\s+[a-z0-9._~+/=-]{4,}|\b(?:password|passwd|pwd|secret|token|api[_-]?key|authorization|cookie)\s*[:=]\s*[^\s,;]+|\beyj[a-z0-9_-]{8,}\.[a-z0-9_-]{8,}\.[a-z0-9_-]{4,}\b|\b(?:sk-|ms_|mstd_)[a-z0-9_-]{8,}\b|[a-z0-9.!#$%&'*+/=?^_`{|}~-]+@(?:[a-z0-9-]+\.)+[a-z]{2,}|-----begin[^\r\n]*(?:private key|certificate)-----"#,
        )
        .expect("static sensitive telemetry value regex")
    })
}

fn forbidden_value(value: &str) -> bool {
    value.contains("://") || sensitive_value_regex().is_match(value)
}

fn sanitize_string(value: &mut String, max_string_bytes: usize, stats: &mut SanitizeStats) {
    if forbidden_value(value) {
        value.clear();
        value.push_str("[REDACTED]");
        stats.redacted = stats.redacted.saturating_add(1);
    } else if value.len() > max_string_bytes {
        *value = truncate_utf8(value, max_string_bytes);
        stats.truncated = stats.truncated.saturating_add(1);
    }
}

fn sanitize_value(value: &mut Value, max_string_bytes: usize, stats: &mut SanitizeStats) {
    match value {
        Value::String(text) => sanitize_string(text, max_string_bytes, stats),
        Value::Array(values) => {
            for value in values {
                sanitize_value(value, max_string_bytes, stats);
            }
        }
        Value::Object(values) => sanitize_map(values, max_string_bytes, stats),
        _ => {}
    }
}

fn sanitize_map(map: &mut Map<String, Value>, max_string_bytes: usize, stats: &mut SanitizeStats) {
    map.retain(|key, value| {
        if forbidden_key(key) {
            stats.removed = stats.removed.saturating_add(1);
            return false;
        }
        sanitize_value(value, max_string_bytes, stats);
        true
    });
}

/// Span、进程内日志与审计 payload 共用的递归隐私边界。
///
/// 禁止 key 会被移除；包含凭据、邮件地址、完整 URL 等敏感模式的字符串会整体替换，
/// 其余字符串按 UTF-8 边界截断。调用方可用返回值记录丢弃/截断诊断，但绝不能记录原值。
pub fn sanitize_telemetry_fields(
    fields: &mut Map<String, Value>,
    max_string_bytes: usize,
) -> SanitizeStats {
    let mut stats = SanitizeStats::default();
    sanitize_map(fields, max_string_bytes, &mut stats);
    stats
}

fn sanitize_btree(
    map: &mut BTreeMap<String, Value>,
    max_string_bytes: usize,
    stats: &mut SanitizeStats,
) {
    map.retain(|key, value| {
        if forbidden_key(key) {
            stats.removed = stats.removed.saturating_add(1);
            return false;
        }
        sanitize_value(value, max_string_bytes, stats);
        true
    });
}

fn truncate_attributes(attributes: &mut BTreeMap<String, Value>, maximum: usize) -> u32 {
    if attributes.len() <= maximum {
        return 0;
    }
    let mut keep = HashSet::with_capacity(maximum);
    for key in PRIORITY_ATTRIBUTES {
        if attributes.contains_key(*key) && keep.len() < maximum {
            keep.insert((*key).to_string());
        }
    }
    for key in attributes.keys() {
        if keep.len() == maximum {
            break;
        }
        keep.insert(key.clone());
    }
    let dropped = attributes.len().saturating_sub(keep.len()) as u32;
    attributes.retain(|key, _| keep.contains(key));
    dropped
}

/// 入队前与每个 sink 前都调用同一个函数。它不会保留禁止字段的原值。
pub fn sanitize_and_limit_span(span: &mut CanonicalSpan, limits: TraceLimits) -> SanitizeStats {
    let mut stats = SanitizeStats::default();
    sanitize_btree(
        &mut span.resource.attributes,
        limits.max_string_bytes,
        &mut stats,
    );
    sanitize_btree(
        &mut span.scope.attributes,
        limits.max_string_bytes,
        &mut stats,
    );
    sanitize_btree(&mut span.attributes, limits.max_string_bytes, &mut stats);
    for event in &mut span.events {
        sanitize_string(&mut event.name, limits.max_string_bytes, &mut stats);
        sanitize_btree(&mut event.attributes, limits.max_string_bytes, &mut stats);
    }
    for link in &mut span.links {
        sanitize_btree(&mut link.attributes, limits.max_string_bytes, &mut stats);
    }
    sanitize_string(&mut span.name, limits.max_string_bytes, &mut stats);
    if let Some(schema_url) = span.resource.schema_url.as_mut() {
        sanitize_string(schema_url, limits.max_string_bytes, &mut stats);
    }
    sanitize_string(&mut span.scope.name, limits.max_string_bytes, &mut stats);
    sanitize_string(&mut span.scope.version, limits.max_string_bytes, &mut stats);
    if let Some(schema_url) = span.scope.schema_url.as_mut() {
        sanitize_string(schema_url, limits.max_string_bytes, &mut stats);
    }
    sanitize_string(&mut span.trace_state, limits.max_string_bytes, &mut stats);
    if let Some(message) = span.status_message.as_mut() {
        sanitize_string(message, limits.max_string_bytes, &mut stats);
    }

    let dropped = truncate_attributes(&mut span.attributes, limits.max_attributes_per_span);
    if dropped > 0 {
        span.dropped_attributes_count = span.dropped_attributes_count.saturating_add(dropped);
        span.mark_partial(PartialReason::AttributeLimit);
    }
    if span.events.len() > limits.max_events_per_span {
        let dropped = span.events.len() - limits.max_events_per_span;
        span.events.truncate(limits.max_events_per_span);
        span.dropped_events_count = span.dropped_events_count.saturating_add(dropped as u32);
        span.mark_partial(PartialReason::EventLimit);
    }
    if span.links.len() > limits.max_links_per_span {
        let dropped = span.links.len() - limits.max_links_per_span;
        span.links.truncate(limits.max_links_per_span);
        span.dropped_links_count = span.dropped_links_count.saturating_add(dropped as u32);
        span.mark_partial(PartialReason::LinkLimit);
    }
    if stats.truncated > 0 {
        span.mark_partial(PartialReason::StringLimit);
    }
    span.dropped_attributes_count = span
        .dropped_attributes_count
        .saturating_add(stats.removed)
        .saturating_add(stats.redacted);
    stats
}

fn validate_value(value: &Value, max_string_bytes: usize) -> Result<()> {
    match value {
        Value::String(text) => {
            if text.len() > max_string_bytes {
                return Err(Error::invalid(
                    "canonical telemetry contains an oversized string",
                ));
            }
            if forbidden_value(text) {
                return Err(Error::invalid(
                    "canonical telemetry contains a forbidden value",
                ));
            }
        }
        Value::Array(values) => {
            for value in values {
                validate_value(value, max_string_bytes)?;
            }
        }
        Value::Object(values) => validate_map(values, max_string_bytes)?,
        _ => {}
    }
    Ok(())
}

fn validate_map(map: &Map<String, Value>, max_string_bytes: usize) -> Result<()> {
    for (key, value) in map {
        if forbidden_key(key) {
            return Err(Error::invalid(format!(
                "canonical telemetry contains forbidden attribute `{key}`"
            )));
        }
        validate_value(value, max_string_bytes)?;
    }
    Ok(())
}

/// Validate fields at a persistence/export boundary without mutating them.
pub fn validate_telemetry_fields(
    fields: &Map<String, Value>,
    max_string_bytes: usize,
) -> Result<()> {
    validate_map(fields, max_string_bytes)
}

/// sink 前的第二道不变量检查。
pub fn validate_sink_invariants(span: &CanonicalSpan, limits: TraceLimits) -> Result<()> {
    if span.trace_id.len() != 32 || !span.trace_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::invalid(
            "canonical trace_id must be 32 hexadecimal characters",
        ));
    }
    if span.span_id.len() != 16 || !span.span_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::invalid(
            "canonical span_id must be 16 hexadecimal characters",
        ));
    }
    if span.attributes.len() > limits.max_attributes_per_span
        || span.events.len() > limits.max_events_per_span
        || span.links.len() > limits.max_links_per_span
    {
        return Err(Error::invalid("canonical span exceeds configured limits"));
    }
    for attributes in [
        &span.resource.attributes,
        &span.scope.attributes,
        &span.attributes,
    ] {
        let fields: Map<String, Value> = attributes.clone().into_iter().collect();
        validate_map(&fields, limits.max_string_bytes)?;
    }
    for event in &span.events {
        if event.name.len() > limits.max_string_bytes || forbidden_value(&event.name) {
            return Err(Error::invalid(
                "canonical span contains an invalid event name",
            ));
        }
        let fields: Map<String, Value> = event.attributes.clone().into_iter().collect();
        validate_map(&fields, limits.max_string_bytes)?;
    }
    for link in &span.links {
        let fields: Map<String, Value> = link.attributes.clone().into_iter().collect();
        validate_map(&fields, limits.max_string_bytes)?;
    }
    for text in [
        Some(span.name.as_str()),
        Some(span.trace_state.as_str()),
        Some(span.scope.name.as_str()),
        Some(span.scope.version.as_str()),
        span.resource.schema_url.as_deref(),
        span.scope.schema_url.as_deref(),
        span.status_message.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if text.len() > limits.max_string_bytes || forbidden_value(text) {
            return Err(Error::invalid(
                "canonical span contains an invalid structural string",
            ));
        }
    }
    Ok(())
}

/// 进程内 tracing layer 的最小完成态输入。转换后立即进入完整 CanonicalSpan。
pub struct FinishedSpan {
    pub name: String,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub parent_span_id: Option<String>,
    pub kind: i32,
    pub start_time_unix_nano: u64,
    pub end_time_unix_nano: u64,
    pub status_code: String,
    pub status_message: Option<String>,
}

fn take_btree(
    fields: &mut Map<String, Value>,
    predicate: impl Fn(&str) -> bool,
) -> BTreeMap<String, Value> {
    let keys: Vec<String> = fields
        .keys()
        .filter(|key| predicate(key))
        .cloned()
        .collect();
    keys.into_iter()
        .filter_map(|key| fields.remove(&key).map(|value| (key, value)))
        .collect()
}

/// 保持旧调用点的 RawEvent 接口，但内部只经 CanonicalSpan 生成存储行。
pub fn finished_span_to_event(mut fields: Map<String, Value>, span: FinishedSpan) -> RawEvent {
    let trace_id = span.trace_id.unwrap_or_else(|| "0".repeat(32));
    let span_id = span.span_id.unwrap_or_else(|| "0".repeat(16));
    let mut canonical = CanonicalSpan::new(
        trace_id,
        span_id,
        span.name,
        span.kind,
        span.start_time_unix_nano,
        span.end_time_unix_nano,
    );
    canonical.parent_span_id = span.parent_span_id;
    canonical.status_code = span.status_code;
    canonical.status_message = span.status_message;

    canonical.resource.attributes = take_btree(&mut fields, |key| {
        key.starts_with("service.")
            || key.starts_with("deployment.")
            || key.starts_with("telemetry.sdk.")
            || key.starts_with("process.runtime.")
            || key.starts_with("node.")
            || key.starts_with("cluster.")
            || key.starts_with("cloud.")
    });
    canonical.scope.name = fields
        .remove("scope_name")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    canonical.scope.version = fields
        .remove("scope_version")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    canonical.events = fields
        .remove("events")
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    canonical.links = fields
        .remove("links")
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    canonical.attributes = fields.into_iter().collect();
    sanitize_and_limit_span(&mut canonical, TraceLimits::default());
    canonical.into_raw_event()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn canonical() -> CanonicalSpan {
        CanonicalSpan::new(
            "0123456789abcdef0123456789abcdef".into(),
            "0123456789abcdef".into(),
            "GET /checkout".into(),
            2,
            5_000,
            9_000,
        )
    }

    #[test]
    fn raw_event_round_trip_preserves_canonical_nested_contract() {
        let mut original = canonical();
        original.events.push(CanonicalEvent {
            time_unix_nano: 11,
            name: "retry".into(),
            attributes: BTreeMap::from([("attempt".into(), Value::from(2))]),
            dropped_attributes_count: 1,
        });
        original.links.push(CanonicalLink {
            trace_id: "11111111111111111111111111111111".into(),
            span_id: "2222222222222222".into(),
            trace_state: "vendor=value".into(),
            flags: 1,
            attributes: BTreeMap::new(),
            dropped_attributes_count: 0,
        });
        let row = original.clone().into_raw_event();
        let restored = CanonicalSpan::try_from_raw_event(&row).unwrap();
        assert_eq!(restored, original);
    }

    #[test]
    fn canonical_contract_calculates_duration_and_timestamp() {
        let event = canonical().into_raw_event();
        assert_eq!(event.timestamp.0, 5);
        assert_eq!(event.fields["duration_ns"], 4_000);
        assert_eq!(event.fields["status_code"], "UNSET");
        assert_eq!(event.fields["schema_version"], 1);
        assert_eq!(
            event.fields["semantic_conventions_version"],
            SEMCONV_VERSION
        );
    }

    #[test]
    fn finished_span_keeps_apm_instrumentation_in_resource_attributes() {
        let event = finished_span_to_event(
            Map::from_iter([
                (
                    "deployment.environment.name".into(),
                    Value::String("production".into()),
                ),
                (
                    "telemetry.sdk.language".into(),
                    Value::String("rust".into()),
                ),
                (
                    "telemetry.sdk.name".into(),
                    Value::String("molesignal".into()),
                ),
                (
                    "telemetry.sdk.version".into(),
                    Value::String("0.1.0".into()),
                ),
                ("process.runtime.name".into(), Value::String("rust".into())),
            ]),
            FinishedSpan {
                name: "request".into(),
                trace_id: Some("0123456789abcdef0123456789abcdef".into()),
                span_id: Some("0123456789abcdef".into()),
                parent_span_id: None,
                kind: 1,
                start_time_unix_nano: 1_000,
                end_time_unix_nano: 2_000,
                status_code: "UNSET".into(),
                status_message: None,
            },
        );
        let span = CanonicalSpan::try_from_raw_event(&event).expect("canonical self span");

        assert_eq!(
            span.resource.attributes["deployment.environment.name"],
            "production"
        );
        assert_eq!(span.resource.attributes["telemetry.sdk.language"], "rust");
        assert_eq!(span.resource.attributes["telemetry.sdk.name"], "molesignal");
        assert_eq!(span.resource.attributes["telemetry.sdk.version"], "0.1.0");
        assert_eq!(span.resource.attributes["process.runtime.name"], "rust");
        assert!(!span.attributes.contains_key("telemetry.sdk.language"));
        assert!(!span.attributes.contains_key("process.runtime.name"));
    }

    #[test]
    fn sanitizer_removes_secrets_and_preserves_priority_fields() {
        let mut span = canonical();
        span.attributes
            .insert("authorization".into(), json!("Bearer secret"));
        span.attributes
            .insert("error.type".into(), json!("timeout"));
        span.attributes.insert("z".into(), json!("long-value"));
        let stats = sanitize_and_limit_span(
            &mut span,
            TraceLimits {
                max_attributes_per_span: 1,
                max_string_bytes: 4,
                ..TraceLimits::default()
            },
        );
        assert_eq!(stats.removed, 1);
        assert_eq!(span.attributes.len(), 1);
        assert_eq!(span.attributes["error.type"], "time");
        assert!(span.partial);
        assert!(
            span.partial_reasons
                .contains(&PartialReason::AttributeLimit)
        );
        assert!(span.partial_reasons.contains(&PartialReason::StringLimit));
        validate_sink_invariants(
            &span,
            TraceLimits {
                max_attributes_per_span: 1,
                max_string_bytes: 4,
                ..TraceLimits::default()
            },
        )
        .unwrap();
    }

    #[test]
    fn sanitizer_redacts_sensitive_values_and_nested_forbidden_fields() {
        let mut span = canonical();
        span.status_message =
            Some("request https://collector.invalid/path?api_key=hunter2 failed".into());
        span.attributes.insert(
            "exception.message".into(),
            json!("contact alice@example.com"),
        );
        span.events.push(CanonicalEvent {
            time_unix_nano: 1,
            name: "retry".into(),
            attributes: BTreeMap::from([(
                "nested".into(),
                json!({
                    "authorization": "Bearer deeply-secret",
                    "attempt": 2
                }),
            )]),
            dropped_attributes_count: 0,
        });
        span.links.push(CanonicalLink {
            trace_id: "11111111111111111111111111111111".into(),
            span_id: "2222222222222222".into(),
            trace_state: String::new(),
            flags: 0,
            attributes: BTreeMap::from([("tool.arguments".into(), json!("private"))]),
            dropped_attributes_count: 0,
        });

        let stats = sanitize_and_limit_span(&mut span, TraceLimits::default());

        assert_eq!(span.status_message.as_deref(), Some("[REDACTED]"));
        assert_eq!(span.attributes["exception.message"], "[REDACTED]");
        assert!(
            span.events[0].attributes["nested"]
                .get("authorization")
                .is_none()
        );
        assert_eq!(span.events[0].attributes["nested"]["attempt"], 2);
        assert!(!span.links[0].attributes.contains_key("tool.arguments"));
        assert!(stats.removed >= 2);
        assert!(stats.redacted >= 2);
        assert!(span.dropped_attributes_count >= 4);
        validate_sink_invariants(&span, TraceLimits::default()).unwrap();
    }

    #[test]
    fn sink_invariant_rejects_forbidden_nested_keys_and_values() {
        let mut span = canonical();
        span.attributes
            .insert("nested".into(), json!({"password": "hunter2"}));
        assert!(validate_sink_invariants(&span, TraceLimits::default()).is_err());

        span.attributes
            .insert("nested".into(), json!({"message": "alice@example.com"}));
        assert!(validate_sink_invariants(&span, TraceLimits::default()).is_err());
    }

    #[test]
    fn hostile_nested_values_and_oversized_unicode_satisfy_sink_property_after_sanitizing() {
        let limits = TraceLimits {
            max_attributes_per_span: 16,
            max_events_per_span: 4,
            max_links_per_span: 4,
            max_string_bytes: 63,
            ..TraceLimits::default()
        };
        for index in 0..256 {
            let hostile = match index % 6 {
                0 => format!("Bearer secret-token-{index:08}"),
                1 => format!("password=hunter-{index}"),
                2 => format!("alice+{index}@example.com"),
                3 => format!("https://collector.invalid/private/{index}?api_key=secret"),
                4 => format!("sk-{index:016x}"),
                _ => "-----BEGIN PRIVATE KEY-----".into(),
            };
            let mut span = canonical();
            span.name = hostile.clone();
            span.status_message = Some(hostile.clone());
            span.attributes.insert(
                "nested".into(),
                json!({
                    "safe": hostile,
                    "authorization": format!("Bearer nested-{index:08}"),
                    "oversized": "界".repeat(256),
                }),
            );
            span.events.push(CanonicalEvent {
                time_unix_nano: index,
                name: format!("password=event-{index}"),
                attributes: BTreeMap::from([(
                    "details".into(),
                    json!({"tool.arguments": format!("secret-{index}"), "safe": "ok"}),
                )]),
                dropped_attributes_count: 0,
            });
            span.links.push(CanonicalLink {
                trace_id: "11111111111111111111111111111111".into(),
                span_id: "2222222222222222".into(),
                trace_state: String::new(),
                flags: 0,
                attributes: BTreeMap::from([("detail".into(), json!(hostile))]),
                dropped_attributes_count: 0,
            });

            sanitize_and_limit_span(&mut span, limits);

            validate_sink_invariants(&span, limits).expect("sanitized Span satisfies sink limits");
            let encoded = serde_json::to_string(&span).expect("encode sanitized Span");
            assert!(!encoded.contains("hunter"));
            assert!(!encoded.contains("example.com"));
            assert!(!encoded.contains("collector.invalid"));
            assert!(!encoded.contains("PRIVATE KEY"));
            assert!(!encoded.contains("nested-"));
            assert!(span.partial_reasons.contains(&PartialReason::StringLimit));
        }
    }

    #[test]
    fn digest_distinguishes_conflicting_content() {
        let first = canonical();
        let mut second = first.clone();
        second.parent_span_id = Some("fedcba9876543210".into());
        assert_ne!(first.content_digest(), second.content_digest());
    }

    #[test]
    fn canonical_schema_is_deterministic_and_has_no_legacy_aliases() {
        let first = canonical_trace_schema();
        let second = canonical_trace_schema();
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(&second).unwrap()
        );
        let names: Vec<&str> = first
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect();
        assert!(names.contains(&"links"));
        assert!(names.contains(&"scope"));
        assert!(names.contains(&"sampling_reason"));
        assert!(!names.contains(&"operation_name"));
        assert!(!names.contains(&"service_name"));
    }

    #[test]
    fn keyed_fingerprint_uses_standard_hmac_sha256() {
        let digest = hmac_sha256(&[0x0b; 20], b"Hi There");
        assert_eq!(
            hex::encode(digest),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }
}
