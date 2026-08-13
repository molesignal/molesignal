// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::{ToolRuntime, common::parse_args};
use crate::{app::iam::IamContext, shared::Result};

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    match kind {
        BuiltinToolKind::ListReportTemplates => {
            let _: EmptyArgs = parse_args(arguments)?;
            let custom = runtime.content.report_templates.list(&auth.org_id).await?;
            Ok(ToolResult::json(json!({
                "builtin_templates": builtin_templates(),
                "custom_templates": custom,
            })))
        }
        BuiltinToolKind::ListScheduledReports => {
            let args: ScheduledArgs = parse_args(arguments)?;
            let reports = runtime
                .content
                .scheduled_reports
                .list(&auth.org_id)
                .await?
                .into_iter()
                .filter(|report| !args.enabled_only || report.enabled)
                .collect::<Vec<_>>();
            Ok(ToolResult::json(json!({"scheduled_reports": reports})))
        }
        BuiltinToolKind::GetReportTemplate => {
            let args: IdArgs = parse_args(arguments)?;
            let template =
                runtime
                    .content
                    .report_templates
                    .get(
                        &auth.org_id,
                        &crate::shared::ids::Id(args.template_id.ok_or_else(|| {
                            crate::shared::Error::invalid("template_id is required")
                        })?),
                    )
                    .await?;
            super::common::json_result(&template)
        }
        BuiltinToolKind::GetScheduledReport => {
            let args: IdArgs = parse_args(arguments)?;
            let report =
                runtime
                    .content
                    .scheduled_reports
                    .get(
                        &auth.org_id,
                        &crate::shared::ids::Id(args.report_id.ok_or_else(|| {
                            crate::shared::Error::invalid("report_id is required")
                        })?),
                    )
                    .await?;
            super::common::json_result(&report)
        }
        BuiltinToolKind::ListReportDeliveries => {
            let args: IdArgs = parse_args(arguments)?;
            let report_id = crate::shared::ids::Id(
                args.report_id
                    .ok_or_else(|| crate::shared::Error::invalid("report_id is required"))?,
            );
            runtime
                .content
                .scheduled_reports
                .get(&auth.org_id, &report_id)
                .await?;
            let deliveries = runtime
                .content
                .scheduled_reports
                .list_deliveries(&auth.org_id, &report_id)
                .await?;
            super::common::json_result(&super::common::bounded(deliveries, args.limit, 200))
        }
        _ => unreachable!("reports handler received unrelated tool"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScheduledArgs {
    #[serde(default)]
    enabled_only: bool,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdArgs {
    template_id: Option<String>,
    report_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

/// HTTP 报告模板端点与 Tool Runtime 共用的内置模板单一事实源。
pub(crate) fn builtin_templates() -> Vec<Value> {
    vec![
        template(
            "weekly-platform-health",
            "Weekly platform health",
            "Dashboard PDF covering availability, error rate, and latency.",
            "dashboard",
            "pdf",
            "previous-calendar-week",
        ),
        template(
            "daily-error-digest",
            "Daily error digest",
            "CSV export for recent incidents and high-volume errors.",
            "saved_view",
            "csv",
            "previous-calendar-day",
        ),
        template(
            "monthly-capacity-review",
            "Monthly capacity review",
            "JSON export for storage, intake, and query usage review.",
            "saved_view",
            "json",
            "previous-calendar-month",
        ),
        template(
            "monthly-sla-compliance",
            "Monthly SLA compliance",
            "Dashboard PDF covering service availability, SLA attainment, error-budget burn, and breach risk.",
            "dashboard",
            "pdf",
            "previous-calendar-month",
        ),
    ]
}

fn template(
    id: &str,
    name: &str,
    description: &str,
    target_type: &str,
    format: &str,
    time_range_preset: &str,
) -> Value {
    json!({
        "id": id, "name": name, "description": description,
        "target_type": target_type, "format": format,
        "time_range_preset": time_range_preset, "is_builtin": true,
        "created_at_micros": null, "updated_at_micros": null,
    })
}
