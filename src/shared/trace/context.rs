// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! W3C Trace Context/Baggage 的有界表示。
//!
//! 传播上下文不是授权来源：外部 `org.id` 只是未受信输入，鉴权完成后必须由
//! [`TraceContext::set_authenticated_org`] 覆盖。第三方出站只注入 Trace
//! Context；内部 Baggage 仅发送到调用方明确判定为可信的目标。

use std::{cell::RefCell, collections::BTreeMap, future::Future};

use http::{HeaderMap, HeaderName, HeaderValue};
use rand::TryRng as _;
use serde::{Deserialize, Serialize};
use tonic::metadata::{Ascii, MetadataKey, MetadataMap, MetadataValue};
use uuid::Uuid;

pub const TRACEPARENT: &str = "traceparent";
pub const TRACESTATE: &str = "tracestate";
pub const BAGGAGE: &str = "baggage";
pub const REQUEST_ID: &str = "x-request-id";
pub const TRACE_ID: &str = "x-trace-id";
pub const TRACE_FORCE: &str = "x-molesignal-trace-force";
pub const TRACE_DEBUG_TOKEN: &str = "x-molesignal-trace-debug";

const MAX_TRACESTATE_BYTES: usize = 512;
const MAX_BAGGAGE_BYTES: usize = 512;
const MAX_CORRELATION_ID_BYTES: usize = 128;

tokio::task_local! {
    static ACTIVE_TRACE_CONTEXT: RefCell<TraceContext>;
}

/// 将请求上下文绑定到当前 async task。深层 HTTP/gRPC/queue wrapper 可读取它，
/// 不需要把传播上下文混入授权参数。
pub async fn with_current_trace_context<F: Future>(context: TraceContext, future: F) -> F::Output {
    ACTIVE_TRACE_CONTEXT
        .scope(RefCell::new(context), future)
        .await
}

pub fn current_trace_context() -> Option<TraceContext> {
    ACTIVE_TRACE_CONTEXT
        .try_with(|context| context.borrow().clone())
        .ok()
}

pub fn update_current_trace_context(update: impl FnOnce(&mut TraceContext)) {
    let _ = ACTIVE_TRACE_CONTEXT.try_with(|context| update(&mut context.borrow_mut()));
}

/// bounded in-process child work must opt in explicitly; plain `tokio::spawn` does not inherit
/// task-local context. Queued/delayed work should persist [`SerializedTraceLink`] instead.
pub fn spawn_with_current_trace_context<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    match current_trace_context() {
        Some(context) => tokio::spawn(with_current_trace_context(context, future)),
        None => tokio::spawn(future),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceTrust {
    External,
    Internal,
    DebugToken,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    #[serde(default)]
    pub trace_flags: u8,
    #[serde(default)]
    pub trace_state: String,
    #[serde(default)]
    pub baggage: BTreeMap<String, String>,
    pub request_id: String,
    pub trust: TraceTrust,
    /// 仅作为诊断提示，不能直接映射为 tail-sampler force keep。
    #[serde(default)]
    pub inbound_sampled_hint: bool,
    #[serde(default)]
    pub force_keep: bool,
}

impl TraceContext {
    pub fn new_root(request_id: impl Into<String>) -> Self {
        Self {
            trace_id: new_trace_id(),
            span_id: new_span_id(),
            parent_span_id: None,
            trace_flags: 0,
            trace_state: String::new(),
            baggage: BTreeMap::new(),
            request_id: request_id.into(),
            trust: TraceTrust::External,
            inbound_sampled_hint: false,
            force_keep: false,
        }
    }

    /// 提取父上下文并创建本地 child。格式错误安全退回新 root。
    pub fn extract_http(headers: &HeaderMap, trust: TraceTrust) -> Self {
        let baggage = extract_baggage(
            headers
                .get(BAGGAGE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default(),
        );
        let request_id = validated_or_new_request_id(
            headers
                .get(REQUEST_ID)
                .and_then(|value| value.to_str().ok())
                .or_else(|| baggage.get("request.id").map(String::as_str)),
        );
        let parsed = headers
            .get(TRACEPARENT)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_traceparent);

        let mut context = match parsed {
            Some(parent) => Self {
                trace_id: parent.trace_id,
                span_id: new_span_id(),
                parent_span_id: Some(parent.parent_span_id),
                trace_flags: parent.trace_flags,
                trace_state: valid_tracestate(
                    headers
                        .get(TRACESTATE)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default(),
                )
                .unwrap_or_default(),
                baggage: baggage.clone(),
                request_id,
                trust,
                inbound_sampled_hint: parent.trace_flags & 0x01 == 0x01,
                force_keep: false,
            },
            None => {
                let mut root = Self::new_root(request_id);
                root.trust = trust;
                root.baggage = baggage;
                root
            }
        };
        context.apply_trusted_force(
            headers
                .get(TRACE_FORCE)
                .and_then(|value| value.to_str().ok())
                == Some("1"),
        );
        // Correlation baggage always mirrors the one validated request ID selected above.
        // Tenant org is removed for external callers and supplied only after auth.
        context
            .baggage
            .insert("request.id".into(), context.request_id.clone());
        context.clear_untrusted_org();
        context
    }

    pub fn extract_grpc(metadata: &MetadataMap, trust: TraceTrust) -> Self {
        let mut headers = HeaderMap::new();
        for name in [TRACEPARENT, TRACESTATE, BAGGAGE, REQUEST_ID, TRACE_FORCE] {
            if let Some(value) = metadata.get(name).and_then(|value| value.to_str().ok())
                && let Ok(value) = HeaderValue::from_str(value)
            {
                headers.insert(HeaderName::from_static(name), value);
            }
        }
        Self::extract_http(&headers, trust)
    }

    pub fn set_authenticated_org(&mut self, org_id: &str) {
        self.baggage.insert("org.id".into(), org_id.into());
    }

    pub fn clear_untrusted_org(&mut self) {
        if self.trust == TraceTrust::External {
            self.baggage.remove("org.id");
        }
    }

    pub fn apply_trusted_force(&mut self, requested: bool) {
        self.force_keep =
            requested && matches!(self.trust, TraceTrust::Internal | TraceTrust::DebugToken);
    }

    pub fn child(&self) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            span_id: new_span_id(),
            parent_span_id: Some(self.span_id.clone()),
            trace_flags: self.trace_flags,
            trace_state: self.trace_state.clone(),
            baggage: self.baggage.clone(),
            request_id: self.request_id.clone(),
            trust: self.trust,
            inbound_sampled_hint: self.inbound_sampled_hint,
            force_keep: self.force_keep,
        }
    }

    /// 为队列/重试持久化一个有界 Link。消费端创建新 root，且不得用于授权。
    pub fn serialized_link(&self) -> SerializedTraceLink {
        SerializedTraceLink {
            trace_id: self.trace_id.clone(),
            span_id: self.span_id.clone(),
            trace_state: self.trace_state.clone(),
            request_id: self.request_id.clone(),
        }
    }

    pub fn traceparent(&self) -> String {
        format!(
            "00-{}-{}-{:02x}",
            self.trace_id, self.span_id, self.trace_flags
        )
    }

    pub fn inject_http(&self, headers: &mut HeaderMap, internal_target: bool) {
        insert_http(headers, TRACEPARENT, &self.traceparent());
        if !self.trace_state.is_empty() {
            insert_http(headers, TRACESTATE, &self.trace_state);
        } else {
            headers.remove(TRACESTATE);
        }
        insert_http(headers, REQUEST_ID, &self.request_id);
        if internal_target {
            let baggage = self.baggage_header();
            if !baggage.is_empty() {
                insert_http(headers, BAGGAGE, &baggage);
            } else {
                headers.remove(BAGGAGE);
            }
        } else {
            headers.remove(BAGGAGE);
            headers.remove(TRACE_FORCE);
            headers.remove(TRACE_DEBUG_TOKEN);
        }
    }

    pub fn inject_grpc(&self, metadata: &mut MetadataMap, internal_target: bool) {
        insert_metadata(metadata, TRACEPARENT, &self.traceparent());
        if !self.trace_state.is_empty() {
            insert_metadata(metadata, TRACESTATE, &self.trace_state);
        } else {
            metadata.remove(TRACESTATE);
        }
        insert_metadata(metadata, REQUEST_ID, &self.request_id);
        if internal_target {
            let baggage = self.baggage_header();
            if !baggage.is_empty() {
                insert_metadata(metadata, BAGGAGE, &baggage);
            } else {
                metadata.remove(BAGGAGE);
            }
        } else {
            metadata.remove(BAGGAGE);
            metadata.remove(TRACE_FORCE);
            metadata.remove(TRACE_DEBUG_TOKEN);
        }
    }

    pub fn attach_to_grpc_response<T>(&self, response: &mut tonic::Response<T>) {
        insert_metadata(response.metadata_mut(), TRACE_ID, &self.trace_id);
        insert_metadata(response.metadata_mut(), REQUEST_ID, &self.request_id);
    }

    fn baggage_header(&self) -> String {
        ["org.id", "request.id"]
            .into_iter()
            .filter_map(|key| self.baggage.get(key).map(|value| format!("{key}={value}")))
            .collect::<Vec<_>>()
            .join(",")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerializedTraceLink {
    pub trace_id: String,
    pub span_id: String,
    #[serde(default)]
    pub trace_state: String,
    #[serde(default)]
    pub request_id: String,
}

impl SerializedTraceLink {
    pub fn new_execution_root(&self) -> TraceContext {
        let mut context =
            TraceContext::new_root(validated_or_new_request_id(Some(&self.request_id)));
        context.trace_state = self.trace_state.clone();
        context
    }
}

/// Create a new execution Trace linked to queued/delayed producer context.
/// `operation` is a static bounded catalog value supplied by the worker.
pub fn linked_execution_root(
    link: Option<&SerializedTraceLink>,
    operation: &'static str,
) -> (TraceContext, tracing::Span) {
    let context = link
        .map(SerializedTraceLink::new_execution_root)
        .unwrap_or_else(|| TraceContext::new_root(Uuid::now_v7().to_string()));
    let links = link
        .map(|link| {
            serde_json::json!([{
                "trace_id": link.trace_id,
                "span_id": link.span_id,
                "trace_state": link.trace_state,
                "flags": 0,
                "attributes": {},
                "dropped_attributes_count": 0
            }])
            .to_string()
        })
        .unwrap_or_else(|| "[]".into());
    let span = tracing::info_span!(
        parent: None,
        "async.execution",
        otel.name = operation,
        otel.kind = "consumer",
        otel.trace_id = %context.trace_id,
        otel.span_id = %context.span_id,
        molesignal.async.operation = operation,
        links = %links,
    );
    (context, span)
}

struct ParsedTraceparent {
    trace_id: String,
    parent_span_id: String,
    trace_flags: u8,
}

fn parse_traceparent(value: &str) -> Option<ParsedTraceparent> {
    if value.len() != 55 {
        return None;
    }
    let mut parts = value.split('-');
    let version = parts.next()?;
    let trace_id = parts.next()?;
    let parent_span_id = parts.next()?;
    let flags = parts.next()?;
    if parts.next().is_some()
        || version.len() != 2
        || version.eq_ignore_ascii_case("ff")
        || !is_lower_hex(version)
        || trace_id.len() != 32
        || !is_lower_hex(trace_id)
        || trace_id.bytes().all(|byte| byte == b'0')
        || parent_span_id.len() != 16
        || !is_lower_hex(parent_span_id)
        || parent_span_id.bytes().all(|byte| byte == b'0')
        || flags.len() != 2
        || !is_lower_hex(flags)
    {
        return None;
    }
    Some(ParsedTraceparent {
        trace_id: trace_id.into(),
        parent_span_id: parent_span_id.into(),
        trace_flags: u8::from_str_radix(flags, 16).ok()?,
    })
}

fn is_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_tracestate(value: &str) -> Option<String> {
    if value.len() > MAX_TRACESTATE_BYTES
        || !value
            .bytes()
            .all(|byte| byte == b'\t' || (0x20..=0x7e).contains(&byte))
    {
        return None;
    }
    Some(value.trim().to_string())
}

fn extract_baggage(value: &str) -> BTreeMap<String, String> {
    if value.len() > MAX_BAGGAGE_BYTES {
        return BTreeMap::new();
    }
    value
        .split(',')
        .filter_map(|member| {
            let pair = member.trim().split(';').next()?;
            let (key, value) = pair.split_once('=')?;
            let key = key.trim();
            let value = value.trim();
            if !matches!(key, "org.id" | "request.id") || !valid_correlation_value(value) {
                return None;
            }
            Some((key.to_string(), value.to_string()))
        })
        .collect()
}

fn valid_correlation_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CORRELATION_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

pub fn valid_request_id(value: &str) -> bool {
    valid_correlation_value(value)
}

pub fn validated_or_new_request_id(value: Option<&str>) -> String {
    value
        .filter(|value| valid_request_id(value))
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::now_v7().to_string())
}

pub(crate) fn new_trace_id() -> String {
    Uuid::now_v7().simple().to_string()
}

pub(crate) fn new_span_id() -> String {
    loop {
        let mut bytes = [0_u8; 8];
        rand::rngs::SysRng
            .try_fill_bytes(&mut bytes)
            .expect("operating-system random source");
        if bytes != [0; 8] {
            return hex::encode(bytes);
        }
    }
}

fn insert_http(headers: &mut HeaderMap, name: &'static str, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
        headers.insert(HeaderName::from_static(name), value);
    }
}

fn insert_metadata(metadata: &mut MetadataMap, name: &'static str, value: &str) {
    if let Ok(value) = MetadataValue::<Ascii>::try_from(value) {
        metadata.insert(MetadataKey::from_static(name), value);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn headers(traceparent: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(TRACEPARENT, traceparent.parse().unwrap());
        headers
    }

    fn generated_ascii(mut state: u64, length: usize) -> String {
        const ALPHABET: &[u8] =
            b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_,.;=@/?:";
        (0..length)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                ALPHABET[(state as usize) % ALPHABET.len()] as char
            })
            .collect()
    }

    #[test]
    fn generated_trace_ids_are_simple_uuid_v7_values() {
        let mut seen = HashSet::new();

        for _ in 0..128 {
            let trace_id = new_trace_id();
            let parsed = Uuid::parse_str(&trace_id).expect("generated trace ID must be a UUID");

            assert_eq!(trace_id.len(), 32);
            assert!(is_lower_hex(&trace_id));
            assert_eq!(parsed.get_version_num(), 7);
            assert_eq!(parsed.simple().to_string(), trace_id);
            assert!(seen.insert(trace_id), "generated trace IDs must be unique");
        }
    }

    #[test]
    fn generated_span_ids_are_random_nonzero_eight_byte_values() {
        let mut seen = HashSet::new();
        let mut values = Vec::new();

        for _ in 0..128 {
            let span_id = new_span_id();
            let value =
                u64::from_str_radix(&span_id, 16).expect("generated span ID must be hexadecimal");

            assert_eq!(span_id.len(), 16);
            assert!(is_lower_hex(&span_id));
            assert_ne!(value, 0);
            assert!(seen.insert(span_id), "generated span IDs must be unique");
            values.push(value);
        }

        assert!(
            values
                .windows(2)
                .any(|pair| pair[1] != pair[0].wrapping_add(1)),
            "generated span IDs must not use an incrementing sequence"
        );
    }

    #[test]
    fn valid_parent_creates_child_without_trusting_sampled_bit() {
        let context = TraceContext::extract_http(
            &headers("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"),
            TraceTrust::External,
        );
        assert_eq!(context.trace_id, "0af7651916cd43dd8448eb211c80319c");
        assert_eq!(context.parent_span_id.as_deref(), Some("b7ad6b7169203331"));
        assert!(context.inbound_sampled_hint);
        assert!(!context.force_keep);
    }

    #[test]
    fn malformed_parent_is_replaced() {
        for malformed in [
            "nope",
            "00-00000000000000000000000000000000-b7ad6b7169203331-01",
            "00-0AF7651916CD43DD8448EB211C80319C-b7ad6b7169203331-01",
            "ff-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
        ] {
            let context = TraceContext::extract_http(&headers(malformed), TraceTrust::External);
            assert!(context.parent_span_id.is_none(), "{malformed}");
            assert_eq!(context.trace_id.len(), 32);
        }
    }

    #[test]
    fn baggage_is_whitelisted_and_authenticated_org_wins() {
        let mut headers = headers("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-00");
        headers.insert(
            BAGGAGE,
            "org.id=spoof,request.id=req-1,user.email=a@example.com,secret=oops"
                .parse()
                .unwrap(),
        );
        let mut context = TraceContext::extract_http(&headers, TraceTrust::External);
        context.set_authenticated_org("real-org");
        assert_eq!(
            context.baggage.get("org.id").map(String::as_str),
            Some("real-org")
        );
        assert_eq!(
            context.baggage.get("request.id").map(String::as_str),
            Some("req-1")
        );
        assert_eq!(context.baggage.len(), 2);
    }

    #[test]
    fn third_party_injection_strips_internal_baggage_and_force_headers() {
        let mut context = TraceContext::new_root("request-1");
        context.set_authenticated_org("org-1");
        context
            .baggage
            .insert("request.id".into(), "request-1".into());
        let mut headers = HeaderMap::new();
        headers.insert(TRACE_FORCE, "1".parse().unwrap());
        headers.insert(TRACE_DEBUG_TOKEN, "secret".parse().unwrap());
        context.inject_http(&mut headers, false);
        assert!(headers.contains_key(TRACEPARENT));
        assert!(!headers.contains_key(BAGGAGE));
        assert!(!headers.contains_key(TRACE_FORCE));
        assert!(!headers.contains_key(TRACE_DEBUG_TOKEN));
    }

    #[test]
    fn queued_execution_starts_a_new_root_without_inheriting_authorization() {
        let mut producer = TraceContext::new_root("request-1");
        producer.set_authenticated_org("org-secret");
        let link = producer.serialized_link();
        let (consumer, _span) = linked_execution_root(Some(&link), "search_job");

        assert_ne!(consumer.trace_id, producer.trace_id);
        assert!(consumer.parent_span_id.is_none());
        assert!(!consumer.baggage.contains_key("org.id"));
        assert_eq!(consumer.request_id, producer.request_id);
    }

    #[test]
    fn only_trusted_callers_can_force_retention() {
        let mut external = TraceContext::new_root("r");
        external.apply_trusted_force(true);
        assert!(!external.force_keep);

        external.trust = TraceTrust::Internal;
        external.apply_trusted_force(true);
        assert!(external.force_keep);
    }

    #[test]
    fn invalid_request_id_is_regenerated() {
        let generated = validated_or_new_request_id(Some("bad request\n"));
        assert_ne!(generated, "bad request\n");
        assert!(valid_request_id(&generated));
    }

    #[test]
    fn hostile_context_headers_are_bounded_and_never_become_authorization_property() {
        for case in 0..512_u64 {
            let candidate = generated_ascii(case + 1, (case as usize * 37) % 600);
            let mut headers = HeaderMap::new();
            if let Ok(value) = HeaderValue::from_str(&candidate) {
                headers.insert(TRACEPARENT, value.clone());
                headers.insert(TRACESTATE, value.clone());
                headers.insert(REQUEST_ID, value);
            }
            let baggage = format!(
                "org.id={candidate},request.id={candidate},\
                 authorization=Bearer-secret,user.email=alice@example.com"
            );
            if let Ok(value) = HeaderValue::from_str(&baggage) {
                headers.insert(BAGGAGE, value);
            }
            headers.insert(TRACE_FORCE, HeaderValue::from_static("1"));

            let context = TraceContext::extract_http(&headers, TraceTrust::External);

            assert_eq!(context.trace_id.len(), 32);
            assert!(is_lower_hex(&context.trace_id));
            assert_eq!(context.span_id.len(), 16);
            assert!(is_lower_hex(&context.span_id));
            assert!(context.trace_state.len() <= MAX_TRACESTATE_BYTES);
            assert!(valid_request_id(&context.request_id));
            assert!(!context.force_keep);
            assert!(!context.baggage.contains_key("org.id"));
            assert!(
                context
                    .baggage
                    .iter()
                    .all(|(key, value)| key == "request.id" && valid_correlation_value(value))
            );
        }
    }
}
