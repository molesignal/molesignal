// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Privacy-safe projection from a sanitized canonical span into the compact
//! APM fact contract.

use std::{collections::BTreeMap, sync::OnceLock};

use regex::Regex;
use serde_json::Value;

use crate::{
    domain::apm::{
        APM_FACT_SCHEMA_VERSION, ApmOutcome, ApmSpanFact, ApmSpanKind, DependencyCategory,
        DependencyIdentity, ErrorIdentity, InstrumentationMetadata, ProtocolStatus,
        SanitizedException, ServiceIdentity, TransactionIdentity, TransactionKind,
    },
    shared::{
        ids::Id,
        time::TimestampMicros,
        trace_normalization::{CanonicalEvent, CanonicalSpan, optional_hmac_fingerprint},
    },
};

const MAX_DIMENSION_BYTES: usize = 192;
const MAX_EXCEPTION_MESSAGE_BYTES: usize = 512;
const MAX_STACK_FRAMES: usize = 16;
const MAX_STACK_FRAME_BYTES: usize = 256;

pub fn extract_apm_fact(org_id: &str, span: &CanonicalSpan) -> Option<ApmSpanFact> {
    let org_id = bounded_text(org_id, MAX_DIMENSION_BYTES)?;
    let resource = &span.resource.attributes;
    let namespace = safe_resource_dimension(resource, "service.namespace");
    let service_name = safe_resource_dimension(resource, "service.name");
    let preferred_environment = safe_resource_dimension(resource, "deployment.environment.name");
    let legacy_environment = safe_resource_dimension(resource, "deployment.environment");
    let service = ServiceIdentity::new(
        namespace.as_deref(),
        service_name.as_deref(),
        preferred_environment.as_deref(),
        legacy_environment.as_deref(),
    );
    let span_kind = map_span_kind(span.kind);
    let transaction = transaction_identity(span);
    let dependency = dependency_identity(span, span_kind);
    let protocol = ProtocolStatus {
        otel_status: span.status_code.clone(),
        http_status_code: integer_attr(span, &["http.response.status_code", "http.status_code"])
            .and_then(|value| u16::try_from(value).ok()),
        rpc_status_code: integer_attr(span, &["rpc.grpc.status_code", "rpc.response.status_code"])
            .and_then(|value| i32::try_from(value).ok()),
    };
    let exception = normalized_exception(span);
    let outcome = if exception.is_some() {
        ApmOutcome::Error
    } else {
        protocol.outcome()
    };
    let error = (outcome == ApmOutcome::Error)
        .then(|| error_identity(span, transaction.as_ref(), exception.as_ref(), &protocol));

    Some(ApmSpanFact {
        schema_version: APM_FACT_SCHEMA_VERSION,
        org_id: Id::from_string(org_id),
        service,
        service_version: safe_resource_dimension(resource, "service.version"),
        service_instance_id: safe_resource_dimension(resource, "service.instance.id"),
        instrumentation: InstrumentationMetadata {
            language: safe_resource_dimension(resource, "telemetry.sdk.language")
                .or_else(|| safe_resource_dimension(resource, "process.runtime.name")),
            sdk_name: safe_resource_dimension(resource, "telemetry.sdk.name"),
            sdk_version: safe_resource_dimension(resource, "telemetry.sdk.version"),
        },
        trace_id: bounded_text(&span.trace_id, MAX_DIMENSION_BYTES)?,
        span_id: bounded_text(&span.span_id, MAX_DIMENSION_BYTES)?,
        parent_span_id: span
            .parent_span_id
            .as_deref()
            .and_then(|value| bounded_text(value, MAX_DIMENSION_BYTES)),
        event_time: TimestampMicros(
            i64::try_from(span.start_time_unix_nano / 1_000).unwrap_or(i64::MAX),
        ),
        duration_micros: span.duration_ns / 1_000,
        span_kind,
        outcome,
        transaction,
        dependency,
        error,
        exception,
    })
}

fn map_span_kind(kind: i32) -> ApmSpanKind {
    match kind {
        1 => ApmSpanKind::Internal,
        2 => ApmSpanKind::Server,
        3 => ApmSpanKind::Client,
        4 => ApmSpanKind::Producer,
        5 => ApmSpanKind::Consumer,
        _ => ApmSpanKind::Unspecified,
    }
}

fn transaction_identity(span: &CanonicalSpan) -> Option<TransactionIdentity> {
    if !matches!(
        map_span_kind(span.kind),
        ApmSpanKind::Server | ApmSpanKind::Consumer | ApmSpanKind::Unspecified
    ) {
        return None;
    }
    if let Some(method) = safe_attr(span, &["http.request.method", "http.method"]) {
        let route = safe_attr(span, &["http.route"]);
        return Some(TransactionIdentity {
            name: route.map_or(method.clone(), |route| format!("{method} {route}")),
            kind: TransactionKind::Http,
        });
    }
    if let Some(service) = safe_attr(span, &["rpc.service"]) {
        let method = safe_attr(span, &["rpc.method"]);
        return Some(TransactionIdentity {
            name: method.map_or(service.clone(), |method| format!("{service}/{method}")),
            kind: TransactionKind::Rpc,
        });
    }
    if let Some(operation) = safe_attr(span, &["messaging.operation.name", "messaging.operation"]) {
        let destination = safe_attr(
            span,
            &[
                "messaging.destination.template",
                "messaging.destination.name",
            ],
        );
        return Some(TransactionIdentity {
            name: destination.map_or(operation.clone(), |target| format!("{operation} {target}")),
            kind: TransactionKind::Messaging,
        });
    }
    safe_low_cardinality_name(&span.name).map(|name| TransactionIdentity {
        name,
        kind: TransactionKind::Span,
    })
}

fn dependency_identity(span: &CanonicalSpan, kind: ApmSpanKind) -> Option<DependencyIdentity> {
    if !matches!(kind, ApmSpanKind::Client | ApmSpanKind::Producer) {
        return None;
    }
    if let Some(target) = safe_attr(span, &["peer.service"]) {
        return Some(DependencyIdentity {
            category: DependencyCategory::Service,
            target,
            operation: safe_low_cardinality_name(&span.name),
        });
    }
    if let Some(system) = safe_attr(span, &["db.system.name", "db.system"]) {
        let namespace = safe_attr(span, &["db.namespace", "db.name", "db.collection.name"])
            .and_then(|value| optional_hmac_fingerprint(&value));
        let target = namespace.map_or(system.clone(), |hash| format!("{system}:{hash}"));
        let category = if matches!(system.as_str(), "redis" | "memcached" | "valkey") {
            DependencyCategory::Cache
        } else {
            DependencyCategory::Database
        };
        return Some(DependencyIdentity {
            category,
            target,
            operation: safe_attr(span, &["db.operation.name", "db.operation"]),
        });
    }
    if let Some(system) = safe_attr(span, &["messaging.system"]) {
        return Some(DependencyIdentity {
            category: DependencyCategory::Messaging,
            target: safe_attr(
                span,
                &[
                    "messaging.destination.template",
                    "messaging.destination.name",
                ],
            )
            .unwrap_or(system),
            operation: safe_attr(span, &["messaging.operation.name", "messaging.operation"]),
        });
    }
    if let Some(service) = safe_attr(span, &["rpc.service"]) {
        return Some(DependencyIdentity {
            category: DependencyCategory::ExternalRpc,
            target: service,
            operation: safe_attr(span, &["rpc.method"]),
        });
    }
    let host = safe_attr(
        span,
        &["server.address", "network.peer.address", "net.peer.name"],
    );
    if host.is_some()
        || string_attr(&span.attributes, "http.request.method").is_some()
        || string_attr(&span.attributes, "http.method").is_some()
    {
        return Some(DependencyIdentity {
            category: DependencyCategory::ExternalHttp,
            target: host.unwrap_or_else(|| crate::domain::apm::OTHER_DIMENSION.to_owned()),
            operation: safe_attr(span, &["http.request.method", "http.method"]),
        });
    }
    Some(DependencyIdentity {
        category: DependencyCategory::Other,
        target: crate::domain::apm::OTHER_DIMENSION.to_owned(),
        operation: safe_low_cardinality_name(&span.name),
    })
}

fn normalized_exception(span: &CanonicalSpan) -> Option<SanitizedException> {
    let event = span
        .events
        .iter()
        .find(|event| event.name.eq_ignore_ascii_case("exception"));
    let error_type = event
        .and_then(|event| event_string(event, "exception.type"))
        .or_else(|| safe_attr(span, &["exception.type", "error.type"]))?;
    let message = event
        .and_then(|event| event_string(event, "exception.message"))
        .or_else(|| {
            string_attr(&span.attributes, "exception.message").and_then(normalize_exception_message)
        });
    let stack = event
        .and_then(|event| event_string(event, "exception.stacktrace"))
        .or_else(|| {
            string_attr(&span.attributes, "exception.stacktrace").and_then(bounded_stack_text)
        })
        .unwrap_or_default();
    Some(SanitizedException {
        error_type,
        message,
        stack_frames: normalize_stack_frames(&stack),
    })
}

fn error_identity(
    span: &CanonicalSpan,
    transaction: Option<&TransactionIdentity>,
    exception: Option<&SanitizedException>,
    protocol: &ProtocolStatus,
) -> ErrorIdentity {
    let error_type = exception
        .map(|value| value.error_type.clone())
        .or_else(|| safe_attr(span, &["error.type"]))
        .unwrap_or_else(|| {
            if let Some(code) = protocol.http_status_code {
                format!("HTTP {code}")
            } else if let Some(code) = protocol.rpc_status_code {
                format!("RPC {code}")
            } else {
                "Error".to_owned()
            }
        });
    let application_frame = exception
        .and_then(|value| value.stack_frames.first())
        .cloned();
    let transaction_name = transaction.map(|value| value.name.clone());
    let seed = format!(
        "{}\u{0}{}\u{0}{}",
        error_type,
        application_frame.as_deref().unwrap_or_default(),
        transaction_name.as_deref().unwrap_or_default()
    );
    ErrorIdentity {
        fingerprint: blake3::hash(seed.as_bytes()).to_hex()[..32].to_owned(),
        error_type,
        application_frame,
        transaction_name,
        overflow: false,
    }
}

fn string_attr<'a>(attributes: &'a BTreeMap<String, Value>, key: &str) -> Option<&'a str> {
    attributes.get(key).and_then(Value::as_str).map(str::trim)
}

fn integer_attr(span: &CanonicalSpan, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| {
        span.attributes
            .get(*key)
            .or_else(|| span.resource.attributes.get(*key))
            .and_then(|value| {
                value
                    .as_i64()
                    .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
                    .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
            })
    })
}

fn safe_attr(span: &CanonicalSpan, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        span.attributes
            .get(*key)
            .or_else(|| span.resource.attributes.get(*key))
            .and_then(Value::as_str)
            .and_then(safe_dimension)
    })
}

fn safe_resource_dimension(attributes: &BTreeMap<String, Value>, key: &str) -> Option<String> {
    string_attr(attributes, key).and_then(safe_dimension)
}

fn safe_dimension(value: &str) -> Option<String> {
    let value = bounded_text(value, MAX_DIMENSION_BYTES)?;
    let lower = value.to_ascii_lowercase();
    if value == "[REDACTED]"
        || value.contains("://")
        || value.contains('?')
        || value.contains('&')
        || lower.contains("authorization")
        || lower.contains("cookie=")
        || lower.contains("password")
        || lower.contains("select ")
        || lower.contains("insert ")
        || lower.contains("update ")
        || lower.contains("delete ")
    {
        return None;
    }
    Some(value)
}

fn safe_low_cardinality_name(value: &str) -> Option<String> {
    let value = safe_dimension(value)?;
    if uuid_or_long_number_regex().is_match(&value) {
        return None;
    }
    Some(value)
}

fn bounded_text(value: &str, max_bytes: usize) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > max_bytes {
        return None;
    }
    Some(value.to_owned())
}

fn event_string(event: &CanonicalEvent, key: &str) -> Option<String> {
    let value = event.attributes.get(key)?.as_str()?;
    match key {
        "exception.message" => normalize_exception_message(value),
        "exception.stacktrace" => bounded_stack_text(value),
        _ => safe_dimension(value),
    }
}

fn normalize_exception_message(value: &str) -> Option<String> {
    let value = bounded_text(value, MAX_EXCEPTION_MESSAGE_BYTES)?;
    if value == "[REDACTED]" || value.contains("://") || value.contains('?') {
        return None;
    }
    let masked = volatile_exception_regex().replace_all(&value, "#");
    safe_dimension(masked.as_ref())
}

fn bounded_stack_text(value: &str) -> Option<String> {
    if value.len() > MAX_STACK_FRAMES * MAX_STACK_FRAME_BYTES * 4 {
        return None;
    }
    Some(value.to_owned())
}

fn normalize_stack_frames(stack: &str) -> Vec<String> {
    stack
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.contains("://") || line.contains('?') {
                return None;
            }
            let mut end = line.len().min(MAX_STACK_FRAME_BYTES);
            while end > 0 && !line.is_char_boundary(end) {
                end -= 1;
            }
            safe_dimension(&line[..end])
        })
        .take(MAX_STACK_FRAMES)
        .collect()
}

fn uuid_or_long_number_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)(?:[0-9a-f]{8}-[0-9a-f-]{27,}|(?:^|[^0-9])[0-9]{5,}(?:[^0-9]|$))")
            .expect("static APM low-cardinality regex")
    })
}

fn volatile_exception_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)(?:[0-9a-f]{8}-[0-9a-f-]{27,}|\b0x[0-9a-f]+\b|\b[0-9]{3,}\b)")
            .expect("static APM exception normalization regex")
    })
}

#[cfg(test)]
mod tests;
