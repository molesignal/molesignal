// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use arrow::array::RecordBatch;
use serde_json::Value;

use super::arrays::{f64_at, i64_at, json_at, string_any, string_at};
use crate::domain::intake::EVENT_ID_FIELD;

pub const SESSION_COLUMNS: &[&str] = &[
    "_timestamp",
    EVENT_ID_FIELD,
    "session_id",
    "user_id",
    "ip_address",
    "client_ip",
    "ip",
    "started_at_micros",
    "duration_ms",
    "application",
    "service",
    "environment",
    "version",
    "country",
    "browser",
    "device",
    "os",
    "landing_page",
    "last_page",
    "view_count",
    "action_count",
    "error_count",
    "trace_id",
];

pub const ACTION_DETAIL_COLUMNS: &[&str] = &[
    "_timestamp",
    EVENT_ID_FIELD,
    "session_id",
    "ts_micros",
    "type",
    "name",
    "page",
    "url",
    "application",
    "service",
    "environment",
    "version",
    "country",
    "browser",
    "device",
    "os",
    "duration_ms",
    "status",
    "lcp_ms",
    "fid_ms",
    "inp_ms",
    "cls",
    "ttfb_ms",
    "trace_id",
    "parent_span_id",
    "payload",
];

pub const ERROR_DETAIL_COLUMNS: &[&str] = &[
    "_timestamp",
    EVENT_ID_FIELD,
    "fingerprint",
    "message",
    "user_id",
    "session_id",
    "page",
    "version",
    "error_type",
    "application",
    "service",
    "environment",
    "error",
];

#[derive(Clone, Copy, Debug, Default)]
pub struct RumScope<'a> {
    pub application: Option<&'a str>,
    pub environment: Option<&'a str>,
    pub version: Option<&'a str>,
    pub country: Option<&'a str>,
    pub device: Option<&'a str>,
}

impl RumScope<'_> {
    fn matches(
        self,
        application: Option<&str>,
        environment: Option<&str>,
        version: Option<&str>,
        country: Option<&str>,
        device: Option<&str>,
    ) -> bool {
        matches_value(self.application, application)
            && matches_value(self.environment, environment)
            && matches_value(self.version, version)
            && matches_value(self.country, country)
            && matches_value(self.device, device)
    }
}

fn matches_value(expected: Option<&str>, actual: Option<&str>) -> bool {
    expected.is_none_or(|expected| actual == Some(expected))
}

#[derive(Clone, Debug)]
pub struct RumSessionRecord {
    pub timestamp_micros: i64,
    pub event_id: String,
    pub session_id: String,
    pub user_id: Option<String>,
    pub ip_address: Option<String>,
    pub duration_ms: Option<f64>,
    pub application: Option<String>,
    pub service: Option<String>,
    pub environment: Option<String>,
    pub version: Option<String>,
    pub country: Option<String>,
    pub browser: Option<String>,
    pub device: Option<String>,
    pub os: Option<String>,
    pub landing_page: Option<String>,
    pub last_page: Option<String>,
    pub view_count: Option<i64>,
    pub action_count: Option<i64>,
    pub error_count: i64,
    pub trace_id: Option<String>,
}

impl RumSessionRecord {
    pub(super) fn from_batch(batch: &RecordBatch, row: usize) -> Option<Self> {
        let session_id = string_at(batch, "session_id", row)?.to_string();
        if session_id.is_empty() {
            return None;
        }
        let timestamp_micros =
            i64_at(batch, "_timestamp", row).or_else(|| i64_at(batch, "started_at_micros", row))?;
        Some(Self {
            timestamp_micros,
            event_id: string_at(batch, EVENT_ID_FIELD, row)
                .unwrap_or(&session_id)
                .to_string(),
            session_id,
            user_id: owned(string_at(batch, "user_id", row)),
            ip_address: owned(string_any(batch, &["ip_address", "client_ip", "ip"], row)),
            duration_ms: f64_at(batch, "duration_ms", row),
            application: owned(string_at(batch, "application", row)),
            service: owned(string_at(batch, "service", row)),
            environment: owned(string_at(batch, "environment", row)),
            version: owned(string_at(batch, "version", row)),
            country: owned(string_at(batch, "country", row)),
            browser: owned(string_at(batch, "browser", row)),
            device: owned(string_at(batch, "device", row)),
            os: owned(string_at(batch, "os", row)),
            landing_page: owned(string_at(batch, "landing_page", row)),
            last_page: owned(string_at(batch, "last_page", row)),
            view_count: i64_at(batch, "view_count", row),
            action_count: i64_at(batch, "action_count", row),
            error_count: i64_at(batch, "error_count", row).unwrap_or_default(),
            trace_id: owned(string_at(batch, "trace_id", row)),
        })
    }

    pub fn matches_scope(&self, scope: RumScope<'_>) -> bool {
        scope.matches(
            self.application.as_deref(),
            self.environment.as_deref(),
            self.version.as_deref(),
            self.country.as_deref(),
            self.device.as_deref(),
        )
    }
}

#[derive(Clone, Debug)]
pub struct RumActionRecord {
    pub timestamp_micros: i64,
    pub event_id: String,
    pub session_id: String,
    pub event_type: String,
    pub name: Option<String>,
    pub page: Option<String>,
    pub url: Option<String>,
    pub application: Option<String>,
    pub service: Option<String>,
    pub environment: Option<String>,
    pub version: Option<String>,
    pub country: Option<String>,
    pub browser: Option<String>,
    pub device: Option<String>,
    pub os: Option<String>,
    pub duration_ms: Option<f64>,
    pub status: Option<i64>,
    pub lcp_ms: Option<f64>,
    pub fid_ms: Option<f64>,
    pub inp_ms: Option<f64>,
    pub cls: Option<f64>,
    pub ttfb_ms: Option<f64>,
    pub trace_id: Option<String>,
    pub parent_span_id: Option<String>,
    pub payload: Option<Value>,
}

impl RumActionRecord {
    pub(super) fn from_batch(batch: &RecordBatch, row: usize) -> Option<Self> {
        let timestamp_micros =
            i64_at(batch, "ts_micros", row).or_else(|| i64_at(batch, "_timestamp", row))?;
        let session_id = string_at(batch, "session_id", row)
            .unwrap_or_default()
            .to_string();
        Some(Self {
            timestamp_micros,
            event_id: string_at(batch, EVENT_ID_FIELD, row)
                .unwrap_or_default()
                .to_string(),
            session_id,
            event_type: normalize_event_type(string_at(batch, "type", row).unwrap_or_default()),
            name: owned(string_at(batch, "name", row)),
            page: owned(string_at(batch, "page", row)),
            url: owned(string_at(batch, "url", row)),
            application: owned(string_at(batch, "application", row)),
            service: owned(string_at(batch, "service", row)),
            environment: owned(string_at(batch, "environment", row)),
            version: owned(string_at(batch, "version", row)),
            country: owned(string_at(batch, "country", row)),
            browser: owned(string_at(batch, "browser", row)),
            device: owned(string_at(batch, "device", row)),
            os: owned(string_at(batch, "os", row)),
            duration_ms: f64_at(batch, "duration_ms", row),
            status: i64_at(batch, "status", row),
            lcp_ms: f64_at(batch, "lcp_ms", row),
            fid_ms: f64_at(batch, "fid_ms", row),
            inp_ms: f64_at(batch, "inp_ms", row),
            cls: f64_at(batch, "cls", row),
            ttfb_ms: f64_at(batch, "ttfb_ms", row),
            trace_id: owned(string_at(batch, "trace_id", row)),
            parent_span_id: owned(string_at(batch, "parent_span_id", row)),
            payload: json_at(batch, "payload", row),
        })
    }

    pub fn matches_scope(&self, scope: RumScope<'_>) -> bool {
        scope.matches(
            self.application.as_deref(),
            self.environment.as_deref(),
            self.version.as_deref(),
            self.country.as_deref(),
            self.device.as_deref(),
        )
    }

    pub fn page_key(&self) -> Option<&str> {
        self.page
            .as_deref()
            .filter(|value| !value.is_empty())
            .or_else(|| self.url.as_deref().filter(|value| !value.is_empty()))
    }
}

#[derive(Clone, Debug)]
pub struct RumErrorRecord {
    pub timestamp_micros: i64,
    pub event_id: String,
    pub fingerprint: String,
    pub message: Option<String>,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub page: Option<String>,
    pub version: Option<String>,
    pub error_type: Option<String>,
    pub application: Option<String>,
    pub service: Option<String>,
    pub environment: Option<String>,
    pub error: Option<Value>,
}

impl RumErrorRecord {
    pub(super) fn from_batch(batch: &RecordBatch, row: usize) -> Option<Self> {
        let fingerprint = string_at(batch, "fingerprint", row)?.to_string();
        if fingerprint.is_empty() {
            return None;
        }
        Some(Self {
            timestamp_micros: i64_at(batch, "_timestamp", row)?,
            event_id: string_at(batch, EVENT_ID_FIELD, row)
                .unwrap_or_default()
                .to_string(),
            fingerprint,
            message: owned(string_at(batch, "message", row)),
            user_id: owned(string_at(batch, "user_id", row)),
            session_id: owned(string_at(batch, "session_id", row)),
            page: owned(string_at(batch, "page", row)),
            version: owned(string_at(batch, "version", row)),
            error_type: owned(string_at(batch, "error_type", row)),
            application: owned(string_at(batch, "application", row)),
            service: owned(string_at(batch, "service", row)),
            environment: owned(string_at(batch, "environment", row)),
            error: json_at(batch, "error", row),
        })
    }

    pub fn matches_scope(&self, scope: RumScope<'_>) -> bool {
        scope.matches(
            self.application.as_deref(),
            self.environment.as_deref(),
            self.version.as_deref(),
            None,
            None,
        )
    }
}

fn owned(value: Option<&str>) -> Option<String> {
    value.filter(|value| !value.is_empty()).map(str::to_string)
}

fn normalize_event_type(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}
