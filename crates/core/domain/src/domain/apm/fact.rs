// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use super::{DependencyIdentity, ErrorIdentity, ServiceIdentity, TransactionIdentity};
use crate::shared::{ids::Id, time::TimestampMicros};

pub const APM_FACT_SCHEMA_VERSION: u16 = 1;

fn fact_schema_version() -> u16 {
    APM_FACT_SCHEMA_VERSION
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApmSpanKind {
    Unspecified,
    Internal,
    Server,
    Client,
    Producer,
    Consumer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApmOutcome {
    Unknown,
    Success,
    Error,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolStatus {
    #[serde(default)]
    pub otel_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_status_code: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpc_status_code: Option<i32>,
}

impl ProtocolStatus {
    pub fn outcome(&self) -> ApmOutcome {
        if self.otel_status.eq_ignore_ascii_case("error")
            || self.http_status_code.is_some_and(|code| code >= 500)
            || self.rpc_status_code.is_some_and(|code| code != 0)
        {
            return ApmOutcome::Error;
        }
        if self.otel_status.eq_ignore_ascii_case("ok")
            || self.http_status_code.is_some_and(|code| code < 500)
            || self.rpc_status_code == Some(0)
        {
            return ApmOutcome::Success;
        }
        ApmOutcome::Unknown
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstrumentationMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdk_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdk_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SanitizedException {
    pub error_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default)]
    pub stack_frames: Vec<String>,
}

/// Compact, privacy-safe fact derived before a Trace sampling decision can
/// discard the original candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApmSpanFact {
    #[serde(default = "fact_schema_version")]
    pub schema_version: u16,
    pub org_id: Id,
    pub service: ServiceIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_instance_id: Option<String>,
    #[serde(default)]
    pub instrumentation: InstrumentationMetadata,
    pub trace_id: String,
    pub span_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    pub event_time: TimestampMicros,
    pub duration_micros: u64,
    pub span_kind: ApmSpanKind,
    pub outcome: ApmOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transaction: Option<TransactionIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency: Option<DependencyIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exception: Option<SanitizedException>,
}

impl ApmSpanFact {
    pub fn contributes_service_red(&self) -> bool {
        matches!(self.span_kind, ApmSpanKind::Server | ApmSpanKind::Consumer)
            || (self.span_kind == ApmSpanKind::Unspecified && self.parent_span_id.is_none())
    }

    pub fn contributes_dependency(&self) -> bool {
        matches!(self.span_kind, ApmSpanKind::Client | ApmSpanKind::Producer)
    }
}
