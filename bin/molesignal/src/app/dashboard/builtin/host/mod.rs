// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The dashboard of the hosts Node Agent watches: CPU, load, memory, disk and network from the
//! `host-metrics` stream, and the kernel and hardware events of the `host-events` stream on the
//! same time line. Read-only, and meant to be copied before it is changed.

mod panels;

use kernel::ids::Id;
use serde_json::{Value, json};

use super::install::{AUTHOR, Builtin};
use crate::app::node_agent::METRICS_STREAM;

/// The version of the content of the dashboard this build ships. Raise it with every change of a
/// panel, a variable or a title: an organization whose dashboard is older gets the new one.
pub const VERSION: u32 = 1;

/// When the content of the current version was written; the model carries it so that an
/// installation of the same version is byte for byte the same.
const WRITTEN: &str = "2026-11-25T00:00:00Z";

/// The metric the hosts of the host variable are listed from: every host Node Agent runs on sends
/// its load.
const HOST_METRIC: &str = "molesignal_host_loadavg_load1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostDashboard {
    Overview,
}

impl Builtin for HostDashboard {
    fn uid(self) -> &'static str {
        "host-overview"
    }

    fn title(self) -> &'static str {
        "主机 · 概览"
    }

    fn tags(self) -> Vec<&'static str> {
        vec!["内置", "host", "node-agent"]
    }

    fn version(self) -> u32 {
        VERSION
    }

    fn model(self, id: &Id) -> Value {
        json!({
            "engine": "molesignal-dashboard",
            "schemaVersion": 2,
            "id": id.0,
            "uid": self.uid(),
            "title": self.title(),
            "description": "Node Agent 采集的一台主机：CPU、负载、内存、磁盘与网络，以及同一时间线上的内核与硬件事件（OOM、丢包、重传、网卡状态、RAS、hungtask、softlockup）。指标只取主机本身，不含容器。",
            "tags": self.tags(),
            "editable": false,
            "defaultDashboard": false,
            "timeSettings": {
                "defaultFrom": "now-6h",
                "defaultTo": "now",
                "timezone": "browser",
            },
            "refreshSettings": {
                "enabled": true,
                "mode": "interval",
                "defaultInterval": "1m",
                "allowedIntervals": ["off", "1m", "5m"],
            },
            "interactionSettings": { "cursorSync": "shared_crosshair" },
            "variables": variables(),
            "annotations": [],
            "links": [],
            "layout": { "type": "grid", "columns": 24, "rowHeight": 8, "gap": 8 },
            "elements": panels::panels(),
            "version": 1,
            "createdAt": WRITTEN,
            "updatedAt": WRITTEN,
            "createdBy": AUTHOR,
            "updatedBy": AUTHOR,
            "extensions": { "builtin": { "key": self.uid(), "version": VERSION } },
        })
    }
}

/// The host, chosen among those that sent their load in the time range, and the width of the
/// bars of the event time line. The host travels in the address as `var-host`.
fn variables() -> Value {
    json!([
        {
            "id": "variable-host",
            "name": "host",
            "label": "主机",
            "type": "query",
            "query": {
                "kind": "query",
                "queryType": "label_values",
                "metric": HOST_METRIC,
                "label": "host.name",
                "expression": format!("label_values({HOST_METRIC}, host.name)"),
                "streamName": METRICS_STREAM,
                "streamType": "metrics",
            },
            "multi": false,
            "includeAll": false,
            "hide": "none",
            "refresh": "time_range_change",
        },
        {
            "id": "variable-interval",
            "name": "interval",
            "label": "间隔",
            "type": "interval",
            "options": [
                { "label": "1 分钟", "value": "1 minute" },
                { "label": "5 分钟", "value": "5 minutes", "selected": true },
                { "label": "1 小时", "value": "1 hour" },
            ],
            "defaultValue": "5 minutes",
            "currentValue": "5 minutes",
            "multi": false,
            "includeAll": false,
            "hide": "none",
            "refresh": "never",
        },
    ])
}

#[cfg(test)]
mod tests;
