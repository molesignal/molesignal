// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Log inspections: what Mole Agent checks in a log stream on a schedule or on demand, its runs
//! and their reports, and running one now.

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output};
use crate::{RiskLevel, ToolSpec};

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let (description, input, permissions, tags) = match kind {
        BuiltinToolKind::ListLogInspections => (
            "List the organization's log inspections: the stream each reads, what it looks for, its schedule, whether it is on, and how its last run ended.",
            object_schema(json!({})),
            vec!["agent.use", "streams.query"],
            vec!["Logs", "Inspections"],
        ),
        BuiltinToolKind::ListLogInspectionRuns => (
            "List the recent runs of one log inspection, newest first, with their status, verdict and summary.",
            json!({
                "type": "object", "required": ["inspection_id"],
                "properties": {
                    "inspection_id": {"type": "string", "minLength": 1},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 50, "default": 10}
                }, "additionalProperties": false
            }),
            vec!["agent.use", "streams.query"],
            vec!["Logs", "Inspections", "Runs"],
        ),
        BuiltinToolKind::GetLogInspectionRun => (
            "Read one run of a log inspection: its verdict, findings and the report it wrote.",
            json!({
                "type": "object", "required": ["inspection_id", "run_id"],
                "properties": {
                    "inspection_id": {"type": "string", "minLength": 1},
                    "run_id": {"type": "string", "minLength": 1}
                }, "additionalProperties": false
            }),
            vec!["agent.use", "streams.query"],
            vec!["Logs", "Inspections", "Runs"],
        ),
        BuiltinToolKind::RunLogInspection => (
            "Run one log inspection now. The run is queued, uses the organization's model budget, and is read with get_log_inspection_run once it ends.",
            json!({
                "type": "object", "required": ["inspection_id", "reason", "impact"],
                "properties": {
                    "inspection_id": {"type": "string", "minLength": 1},
                    "reason": {"type": "string", "minLength": 1, "maxLength": 2000},
                    "impact": {"type": "string", "minLength": 1, "maxLength": 2000},
                    "expires_at_micros": {"type": "integer"}
                }, "additionalProperties": false
            }),
            vec!["inspections.manage"],
            vec!["Logs", "Inspections", "Run"],
        ),
        _ => unreachable!("log-inspection catalog received unrelated kind"),
    };

    let tool = ToolSpec::read(
        kind.name(),
        description,
        "observability",
        "log_inspections",
        input,
        open_output(),
        &permissions,
        &tags,
    );

    match kind {
        // A run reads logs and writes a report; it changes nothing it reads.
        BuiltinToolKind::RunLogInspection => tool.managed_mutation(RiskLevel::L1, false, false),
        _ => tool,
    }
}
