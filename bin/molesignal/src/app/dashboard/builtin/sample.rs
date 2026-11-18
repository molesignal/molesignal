// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! An editable starter dashboard. Reinstalling never overwrites an organization's changes.

use domain::dashboard::Dashboard;
use kernel::{Error, Result, ids::Id};
use serde_json::{Value, json};

use super::panel::{Panel, at};
use crate::app::dashboard::DashboardService;

pub const UID: &str = "sample-telemetry-overview";

pub fn model() -> Value {
    let logs = "sample_app_logs";
    let traces = "sample_app_traces";
    let mut latency = Panel::bars(
        "latency", "服务平均耗时", at(0, 8, 12, 12), traces,
        "SELECT \"service.name\" AS service, AVG(duration_ns) / 1000000 AS latency FROM sample_app_traces GROUP BY \"service.name\"".into(),
    ).unit("milliseconds", 1).build();

    latency["queries"][0]["query"]["streamType"] = json!("traces");

    let mut failures = Panel::table(
        "failed-spans", "异常链路", at(12, 8, 12, 12), traces,
        "SELECT trace_id, \"service.name\", duration_ns / 1000000 AS duration FROM sample_app_traces WHERE status_code = 2".into(),
    ).column("duration", "milliseconds", 1).build();

    failures["queries"][0]["query"]["streamType"] = json!("traces");

    let mut metrics = Panel::bars(
        "metrics",
        "采集到的服务延迟指标",
        at(12, 20, 12, 12),
        "sample_app_metrics",
        "SELECT service, AVG(value) AS latency FROM sample_app_metrics GROUP BY service".into(),
    )
    .unit("milliseconds", 1)
    .build();

    metrics["queries"][0]["query"]["streamType"] = json!("metrics");

    json!({
        "engine": "molesignal-dashboard", "schemaVersion": 2,
        "uid": UID, "title": "样例应用观测总览",
        "description": "从错误日志、服务耗时与 trace_id 开始排障。样例数据为一次性导入，可复制或编辑此仪表盘。",
        "tags": ["sample", "onboarding-v1"], "editable": true, "defaultDashboard": false,
        "timeSettings": {"defaultFrom": "now-3h", "defaultTo": "now", "timezone": "browser"},
        "refreshSettings": {"enabled": false, "mode": "off", "defaultInterval": "off", "allowedIntervals": ["off", "1m"]},
        "interactionSettings": {"cursorSync": "shared_crosshair"},
        "variables": [], "annotations": [], "links": [],
        "layout": {"type": "grid", "columns": 24, "rowHeight": 8, "gap": 8},
        "elements": [
            Panel::number("logs", "日志总量", at(0, 0, 12, 8), logs,
                "SELECT COUNT(*) AS value FROM sample_app_logs".into()).build(),
            Panel::bars("errors", "各服务错误数", at(12, 0, 12, 8), logs,
                "SELECT service, COUNT(*) AS errors FROM sample_app_logs WHERE level = 'error' GROUP BY service".into()).build(),
            latency, failures,
            Panel::table("error-logs", "错误日志与关联链路", at(0, 20, 12, 12), logs,
                "SELECT service, message, trace_id FROM sample_app_logs WHERE level = 'error'".into()).build(),
            metrics
        ],
        "extensions": {"starter": {"key": UID, "version": 1}}
    })
}

impl DashboardService {
    pub async fn sample_dashboard(&self, org_id: &Id) -> Result<Dashboard> {
        self.dashboards.get_by_uid(org_id, UID).await
    }

    pub async fn install_sample(&self, org_id: &Id, user_id: &Id) -> Result<Dashboard> {
        match self.sample_dashboard(org_id).await {
            Ok(existing) => return Ok(existing),
            Err(Error::NotFound(_)) => {}
            Err(error) => return Err(error),
        }

        match self
            .create(org_id.clone(), None, user_id.clone(), model())
            .await
        {
            // Concurrent installs converge on the unique organization + uid.
            Err(Error::Conflict(_)) => self.sample_dashboard(org_id).await,
            result => result,
        }
    }
}
