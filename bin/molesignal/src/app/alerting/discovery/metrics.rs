// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 指标的自动发现：只看一组常见的资源指标，指标目录里有哪个名字才查哪个，不扫全部指标。
//! OpenTelemetry 的主机、Kubernetes 指标和 Prometheus 的 node_exporter、kube-state-metrics 都认：
//!
//! - **快满了**（[`FindingKind::MetricSaturation`]）：文件系统使用率 ≥ 90%，内存 ≥ 95%。
//! - **CPU 突变**（[`FindingKind::MetricShift`]）：主机 CPU 使用率现在 ≥ 60%，并比 1 小时前高 30 个
//!   百分点以上。
//! - **容器重启**（[`FindingKind::MetricRestarts`]）：最近 15 分钟重启过。
//!
//! 查询都是即时的；结果按标签在这里聚合（OpenTelemetry 的标签名带点，不写进 PromQL 的 `by`）。

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

use serde_json::{Value, json};

use crate::{
    domain::{
        alerting::{
            discovery::{Detection, FindingKind},
            incident::Severity,
        },
        metrics::MetricDirectory,
        query::{QueryEngine, QueryLanguage, QueryRequest, QueryResult},
    },
    shared::{
        Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

const MINUTE: i64 = 60_000_000;

/// 拿来比的 CPU：1 小时前。
const SHIFT_BACK: i64 = 60 * MINUTE;

/// 一条序列的标签和值。
pub type Series = (BTreeMap<String, String>, f64);

/// 哪些标签说是哪台主机、哪个容器。
const HOST_LABELS: [&str; 6] = [
    "host.name",
    "host_name",
    "host",
    "k8s.node.name",
    "node",
    "instance",
];
const MOUNT_LABELS: [&str; 3] = ["mountpoint", "system.filesystem.mountpoint", "device"];

/// 一项检查：要哪几个指标名在，查什么。
struct Check {
    kind: FindingKind,
    metric: &'static str,
    needs: &'static [&'static str],
    promql: &'static str,
}

const SATURATION: [Check; 4] = [
    Check {
        kind: FindingKind::MetricSaturation,
        metric: "system.filesystem.utilization",
        needs: &["system.filesystem.utilization"],
        promql: r#"{__name__="system.filesystem.utilization"} >= 0.9"#,
    },
    Check {
        kind: FindingKind::MetricSaturation,
        metric: "node_filesystem_avail_bytes",
        needs: &["node_filesystem_avail_bytes", "node_filesystem_size_bytes"],
        promql: r#"1 - node_filesystem_avail_bytes{fstype!~"tmpfs|overlay|squashfs"} / node_filesystem_size_bytes{fstype!~"tmpfs|overlay|squashfs"} >= 0.9"#,
    },
    Check {
        kind: FindingKind::MetricSaturation,
        metric: "system.memory.utilization",
        needs: &["system.memory.utilization"],
        promql: r#"{__name__="system.memory.utilization", state="used"} >= 0.95"#,
    },
    Check {
        kind: FindingKind::MetricSaturation,
        metric: "node_memory_MemAvailable_bytes",
        needs: &[
            "node_memory_MemAvailable_bytes",
            "node_memory_MemTotal_bytes",
        ],
        promql: "1 - node_memory_MemAvailable_bytes / node_memory_MemTotal_bytes >= 0.95",
    },
];

const RESTARTS: [Check; 2] = [
    Check {
        kind: FindingKind::MetricRestarts,
        metric: "kube_pod_container_status_restarts_total",
        needs: &["kube_pod_container_status_restarts_total"],
        promql: "increase(kube_pod_container_status_restarts_total[15m]) >= 1",
    },
    Check {
        kind: FindingKind::MetricRestarts,
        metric: "k8s.container.restarts",
        needs: &["k8s.container.restarts"],
        promql: r#"delta({__name__="k8s.container.restarts"}[15m]) >= 1"#,
    },
];

/// CPU 空闲的比例，按核：现在和 1 小时前各查一次，按主机平均。
const CPU_IDLE: [Check; 2] = [
    Check {
        kind: FindingKind::MetricShift,
        metric: "system.cpu.utilization",
        needs: &["system.cpu.utilization"],
        promql: r#"{__name__="system.cpu.utilization", state="idle"}"#,
    },
    Check {
        kind: FindingKind::MetricShift,
        metric: "node_cpu_seconds_total",
        needs: &["node_cpu_seconds_total"],
        promql: r#"rate(node_cpu_seconds_total{mode="idle"}[5m])"#,
    },
];

/// 一轮读出来的：找到的，和读到了的检查（按种类）。
#[derive(Debug, Default)]
pub struct MetricScan {
    pub detections: Vec<Detection>,
    pub scanned: HashSet<FindingKind>,
}

pub struct MetricDiscovery {
    engine: Arc<dyn QueryEngine>,
    directory: Arc<dyn MetricDirectory>,
}

impl MetricDiscovery {
    pub fn new(engine: Arc<dyn QueryEngine>, directory: Arc<dyn MetricDirectory>) -> Self {
        Self { engine, directory }
    }

    pub async fn detect(&self, org_id: &Id, now: TimestampMicros) -> Result<MetricScan> {
        let names: HashSet<String> = self
            .directory
            .list(org_id)
            .await?
            .into_iter()
            .map(|entry| entry.metric_name)
            .collect();
        let present = |check: &Check| check.needs.iter().all(|name| names.contains(*name));
        let mut scan = MetricScan::default();

        for check in SATURATION
            .iter()
            .chain(RESTARTS.iter())
            .filter(|check| present(check))
        {
            match self.instant(org_id, check.promql, now.0).await {
                Ok(series) => {
                    scan.detections
                        .extend(series.iter().map(|series| detection(check, series)));
                    scan.scanned.insert(check.kind);
                }
                Err(error) => {
                    tracing::debug!(metric = check.metric, %error, "a metric could not be read for discovery")
                }
            }
        }

        for check in CPU_IDLE.iter().filter(|check| present(check)) {
            let read = async {
                let now_idle = self.instant(org_id, check.promql, now.0).await?;
                let before_idle = self
                    .instant(org_id, check.promql, now.0 - SHIFT_BACK)
                    .await?;
                Ok::<_, crate::shared::Error>((now_idle, before_idle))
            };

            match read.await {
                Ok((now_idle, before_idle)) => {
                    scan.detections
                        .extend(cpu_shifts(check, &now_idle, &before_idle));
                    scan.scanned.insert(check.kind);
                }
                Err(error) => {
                    tracing::debug!(metric = check.metric, %error, "CPU could not be read for discovery")
                }
            }
        }

        Ok(scan)
    }

    async fn instant(&self, org_id: &Id, promql: &str, at: i64) -> Result<Vec<Series>> {
        let result = self
            .engine
            .execute(QueryRequest {
                org_id: org_id.clone(),
                language: QueryLanguage::Promql,
                statement: promql.to_string(),
                time_range: TimeRange::new(TimestampMicros(at - 5 * MINUTE), TimestampMicros(at)),
                stream: None,
                limit: Some(500),
                step: None,
                federation_clusters: Vec::new(),
            })
            .await?;

        Ok(series_of(&result))
    }
}

/// 即时结果的每一行：`[value, ...labels]`。
pub fn series_of(result: &QueryResult) -> Vec<Series> {
    let Some(value) = result.columns.iter().position(|column| column == "value") else {
        return Vec::new();
    };

    result
        .rows
        .iter()
        .filter_map(|row| {
            let number = row.get(value).and_then(Value::as_f64)?;
            let labels = result
                .columns
                .iter()
                .zip(row)
                .filter(|(column, _)| column.as_str() != "value" && column.as_str() != "__name__")
                .filter_map(|(column, cell)| {
                    cell.as_str().map(|text| (column.clone(), text.to_string()))
                })
                .collect();

            Some((labels, number))
        })
        .collect()
}

fn first_label<'a>(labels: &'a BTreeMap<String, String>, names: &[&str]) -> Option<&'a String> {
    names.iter().find_map(|name| labels.get(*name))
}

fn host_of(labels: &BTreeMap<String, String>) -> String {
    first_label(labels, &HOST_LABELS)
        .cloned()
        .unwrap_or_else(|| "unknown host".into())
}

/// 容器：命名空间/Pod/容器（哪个标签有就用哪个）。
fn container_of(labels: &BTreeMap<String, String>) -> String {
    [
        first_label(labels, &["namespace", "k8s.namespace.name"]),
        first_label(labels, &["pod", "k8s.pod.name"]),
        first_label(labels, &["container", "k8s.container.name"]),
    ]
    .into_iter()
    .flatten()
    .cloned()
    .collect::<Vec<_>>()
    .join("/")
}

fn detection(check: &Check, (labels, value): &Series) -> Detection {
    let (subject, title, severity) = match check.kind {
        FindingKind::MetricRestarts => {
            let container = container_of(labels);
            let subject = if container.is_empty() {
                host_of(labels)
            } else {
                container
            };
            let severity = if *value >= 3.0 {
                Severity::Error
            } else {
                Severity::Warning
            };
            (
                subject.clone(),
                format!("{subject} restarted {value:.0} times in 15 minutes"),
                severity,
            )
        }
        _ => {
            let host = host_of(labels);
            let subject = match first_label(labels, &MOUNT_LABELS) {
                Some(mount) if !check.metric.contains("memory") => format!("{host} {mount}"),
                _ => host,
            };
            let what = if check.metric.contains("memory") {
                "Memory"
            } else {
                "Disk"
            };
            let severity = if *value >= 0.97 {
                Severity::Error
            } else {
                Severity::Warning
            };
            (
                subject.clone(),
                format!("{what} of {subject} is {:.0}% full", value * 100.0),
                severity,
            )
        }
    };

    Detection {
        kind: check.kind,
        key: format!("{}|{}|{subject}", check.kind.as_str(), check.metric),
        severity,
        subject,
        environment: None,
        metric: Some(check.metric.to_string()),
        title,
        value: Some(*value),
        baseline: None,
        started_at: None,
        detail: json!({ "labels": labels, "promql": check.promql }),
    }
}

/// 每台主机的 CPU 使用率：1 减去各核空闲比例的平均。
fn cpu_by_host(idle: &[Series]) -> HashMap<String, f64> {
    let mut sums: HashMap<String, (f64, f64)> = HashMap::new();

    for (labels, value) in idle {
        let entry = sums.entry(host_of(labels)).or_default();
        entry.0 += value;
        entry.1 += 1.0;
    }

    sums.into_iter()
        .map(|(host, (sum, count))| (host, 1.0 - sum / count.max(1.0)))
        .collect()
}

/// CPU 使用率现在至少 60%、又比 1 小时前高 30 个百分点以上的主机。
fn cpu_shifts(check: &Check, now_idle: &[Series], before_idle: &[Series]) -> Vec<Detection> {
    let before = cpu_by_host(before_idle);
    let mut shifts: Vec<Detection> = cpu_by_host(now_idle)
        .into_iter()
        .filter_map(|(host, now)| {
            let then = *before.get(&host)?;

            (now >= 0.6 && now - then >= 0.3).then(|| Detection {
                kind: FindingKind::MetricShift,
                key: format!("metric_shift|cpu|{host}"),
                severity: if now >= 0.9 {
                    Severity::Error
                } else {
                    Severity::Warning
                },
                subject: host.clone(),
                environment: None,
                metric: Some(check.metric.to_string()),
                title: format!(
                    "CPU of {host} rose from {:.0}% to {:.0}% in an hour",
                    then * 100.0,
                    now * 100.0
                ),
                value: Some(now),
                baseline: Some(then),
                started_at: None,
                detail: json!({ "promql": check.promql }),
            })
        })
        .collect();

    shifts.sort_by(|left, right| left.key.cmp(&right.key));
    shifts
}

#[cfg(test)]
mod tests;
