// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output, time_range_schema};
use crate::ToolSpec;

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let common = json!({
        "time_range": time_range_schema(), "namespace": {"type": "string"},
        "environment": {"type": "string"}, "version": {"type": "string"}
    });
    let (description, input, tags) = match kind {
        BuiltinToolKind::ApmOverview => (
            "Return bounded RED metrics, trends, service health, dependencies, and top errors.",
            object_schema(common.clone()),
            vec!["APM", "RED"],
        ),
        BuiltinToolKind::ListApmServices => {
            let mut fields = common.clone();
            fields["sort"] = json!({"type": "string", "enum": ["request_count", "error_rate", "p95_micros", "name"]});
            fields["limit"] =
                json!({"type": "integer", "minimum": 1, "maximum": 100, "default": 25});
            (
                "List APM services with RED health summaries.",
                object_schema(fields),
                vec!["APM", "Services"],
            )
        }
        BuiltinToolKind::GetApmService => {
            let mut fields = common.clone();
            fields["service"] = json!({"type": "string"});
            (
                "Get detailed RED trends, transactions, dependencies, errors, and versions for one service.",
                json!({"type": "object", "required": ["service"], "properties": fields, "additionalProperties": false}),
                vec!["APM", "Services"],
            )
        }
        BuiltinToolKind::ListApmTransactions => {
            let mut fields = common.clone();
            fields["service"] = json!({"type": "string"});
            fields["limit"] =
                json!({"type": "integer", "minimum": 1, "maximum": 100, "default": 25});
            (
                "List APM transaction groups with RED summaries.",
                object_schema(fields),
                vec!["APM", "Transactions"],
            )
        }
        BuiltinToolKind::GetApmTransaction => {
            let mut fields = common.clone();
            fields["transaction"] = json!({"type": "string"});
            fields["service"] = json!({"type": "string"});
            (
                "Get detailed trends and traces for one APM transaction.",
                json!({"type": "object", "required": ["transaction"], "properties": fields, "additionalProperties": false}),
                vec!["APM", "Transactions"],
            )
        }
        BuiltinToolKind::ListApmDependencies => (
            "List APM service dependencies with latency and error summaries.",
            object_schema(common.clone()),
            vec!["APM", "Dependencies"],
        ),
        BuiltinToolKind::ListApmErrors => {
            let mut fields = common;
            fields["service"] = json!({"type": "string"});
            fields["limit"] =
                json!({"type": "integer", "minimum": 1, "maximum": 100, "default": 25});
            (
                "List grouped APM errors with affected services and trace handles.",
                object_schema(fields),
                vec!["APM", "Errors"],
            )
        }
        BuiltinToolKind::GetApmError => {
            let mut fields = common.clone();
            fields["fingerprint"] = json!({"type": "string"});
            (
                "Get one APM error group with samples and trace handles.",
                json!({"type": "object", "required": ["fingerprint"], "properties": fields, "additionalProperties": false}),
                vec!["APM", "Errors"],
            )
        }
        BuiltinToolKind::CompareApmVersions => {
            let mut fields = common.clone();
            fields["service"] = json!({"type": "string"});
            fields["baseline_version"] = json!({"type": "string"});
            fields["candidate_version"] = json!({"type": "string"});
            (
                "Compare RED metrics and regressions between two service versions.",
                json!({"type": "object", "required": ["service", "baseline_version", "candidate_version"], "properties": fields, "additionalProperties": false}),
                vec!["APM", "Versions"],
            )
        }
        BuiltinToolKind::GetApmHealth => (
            "Return APM projection freshness, coverage, and data-quality health.",
            object_schema(common),
            vec!["APM", "Health"],
        ),
        _ => unreachable!("APM catalog received unrelated kind"),
    };
    ToolSpec::read(
        kind.name(),
        description,
        "observability",
        "apm",
        input,
        open_output(),
        &["streams.query", "sys.telemetry.read"],
        &tags,
    )
    .any_permission()
}
