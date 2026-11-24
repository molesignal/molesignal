// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Tencent Cloud Monitor metrics, pulled with `GetMonitorData` (monitor API 2018-07-24).
//!
//! `GetMonitorData` reads only the instances it is given, at most 50 a call. The one API that
//! filters by dimension conditions instead, `DescribeStatisticData`, takes only the `QCE` and
//! `TKE2` (container) namespaces, so it cannot replace the instance list. The connector reads the
//! instances `instances` names; for `QCE/CVM` with none named it lists the region's instances with
//! CVM `DescribeInstances` once a pull.
//!
//! Each pull asks, per metric and per 50 instances, for the next [`Window`] with every statistic of
//! `statistics` in one call (`SpecifyStatistics`). Calls go one after another, apart enough to stay
//! under the 20 a second the API allows.
//!
//! Settings: `accessKey` (SecretId), `secretKey` (SecretKey), `region`, `namespace`, `metrics`,
//! `statistics`, `instances`, `period`, `delay`; see [`settings`].

mod api;
mod data_points;
mod settings;

use std::time::Duration;

use async_trait::async_trait;
use kernel::{Error, Result, time::TimestampMicros};
use serde_json::{Value, json};
use tokio::sync::OnceCell;

use self::settings::{CVM_NAMESPACE, Instance, Settings};
use crate::infra::connectors::{
    PullBatch, PullConnector, PullContext,
    cloud_metrics::{self, Window},
    repo::Connector,
};

/// How many instances one `GetMonitorData` call takes.
const INSTANCES_PER_CALL: usize = 50;

/// How many CVM instances one `DescribeInstances` page lists.
const INSTANCES_PER_PAGE: usize = 100;

/// How many `DescribeInstances` pages a pull reads at most.
const MAX_INSTANCE_PAGES: usize = 20;

/// How many `GetMonitorData` calls one pull makes at most.
const MAX_CALLS_PER_PULL: usize = 300;

/// The pause between two calls: 20 a second at most.
const REQUEST_GAP: Duration = Duration::from_millis(50);

/// Rejects settings that could never work.
pub fn validate(config: &Value) -> Result<()> {
    Settings::parse(config).map(|_| ())
}

pub struct TencentMonitorMetrics {
    /// The settings; why they are unusable when they are.
    settings: std::result::Result<Settings, String>,
    /// The CVM instances found for this connector, listed once while it lives.
    found: OnceCell<Vec<Instance>>,
}

impl TencentMonitorMetrics {
    pub fn new(connector: &Connector) -> Self {
        Self {
            settings: Settings::parse(&connector.config_json).map_err(|e| e.to_string()),
            found: OnceCell::new(),
        }
    }

    fn settings(&self) -> Result<&Settings> {
        self.settings
            .as_ref()
            .map_err(|reason| Error::invalid(reason.clone()))
    }

    /// `DescribeBaseMetrics` for the first metric: it proves the credentials and that the
    /// namespace has the metric; with no instances named, one `DescribeInstances` of one instance
    /// proves the CVM read permission too. Nothing moves the read position.
    pub async fn check(&self) -> Result<()> {
        let settings = self.settings()?;
        let client = cloud_metrics::http_client()?;
        let metric = &settings.metrics[0];

        let response = api::call(
            &client,
            settings,
            api::MONITOR,
            "DescribeBaseMetrics",
            &json!({ "Namespace": settings.namespace, "MetricName": metric }),
        )
        .await?;

        let known = response
            .get("MetricSet")
            .and_then(Value::as_array)
            .is_some_and(|set| !set.is_empty());

        if !known {
            return Err(Error::not_found(format!(
                "Tencent Cloud Monitor has no metric `{metric}` in {}",
                settings.namespace
            )));
        }

        if settings.instances.is_empty() {
            api::call(
                &client,
                settings,
                api::CVM,
                "DescribeInstances",
                &json!({ "Limit": 1, "Offset": 0 }),
            )
            .await?;
        }

        Ok(())
    }

    /// The instances to read: those the settings name, else the region's CVM instances.
    async fn instances(&self, client: &reqwest::Client) -> Result<&[Instance]> {
        let settings = self.settings()?;

        if !settings.instances.is_empty() {
            return Ok(&settings.instances);
        }

        let found = self
            .found
            .get_or_try_init(|| discover(client, settings))
            .await?;

        Ok(found)
    }
}

#[async_trait]
impl PullConnector for TencentMonitorMetrics {
    async fn run_once(&self, ctx: &PullContext) -> Result<PullBatch> {
        let settings = self.settings()?;
        let now = TimestampMicros::now().0 / 1_000_000;

        let Some(window) = Window::next(settings.schedule, &ctx.position, now) else {
            return Ok(PullBatch::default());
        };

        let client = cloud_metrics::http_client()?;
        let instances = self.instances(&client).await?;
        let mut points = Vec::new();
        let mut calls = 0;
        let mut cut_short = false;

        'metrics: for metric in &settings.metrics {
            for chunk in instances.chunks(INSTANCES_PER_CALL) {
                if calls >= MAX_CALLS_PER_PULL {
                    cut_short = true;
                    break 'metrics;
                }

                if calls > 0 {
                    tokio::time::sleep(REQUEST_GAP).await;
                }

                calls += 1;

                let request =
                    api::monitor_data_request(settings, metric, chunk, window.start, window.end);

                let response =
                    api::call(&client, settings, api::MONITOR, "GetMonitorData", &request).await?;

                points.extend(data_points::points(settings, metric, &response, &window));
            }
        }

        let warning = window.warning().or_else(|| {
            cut_short.then(|| {
                format!(
                    "a pull makes at most {MAX_CALLS_PER_PULL} GetMonitorData calls of \
                     {INSTANCES_PER_CALL} instances: some series of this range were not read; \
                     name fewer metrics or instances"
                )
            })
        });

        Ok(PullBatch {
            metrics: cloud_metrics::events(points),
            position: Some(window.position()),
            more: window.more,
            warning,
            ..PullBatch::default()
        })
    }
}

/// Every CVM instance of the region, as `InstanceId` dimensions, a page at a time.
async fn discover(client: &reqwest::Client, settings: &Settings) -> Result<Vec<Instance>> {
    debug_assert_eq!(settings.namespace, CVM_NAMESPACE);

    let mut instances = Vec::new();

    for page in 0..MAX_INSTANCE_PAGES {
        if page > 0 {
            tokio::time::sleep(REQUEST_GAP).await;
        }

        let response = api::call(
            client,
            settings,
            api::CVM,
            "DescribeInstances",
            &json!({ "Limit": INSTANCES_PER_PAGE, "Offset": page * INSTANCES_PER_PAGE }),
        )
        .await?;

        let listed = instance_ids(&response);
        let total = response.get("TotalCount").and_then(Value::as_u64);
        let last = listed.len() < INSTANCES_PER_PAGE;

        instances.extend(
            listed
                .into_iter()
                .map(|id| vec![("InstanceId".to_string(), id)]),
        );

        if last || total.is_some_and(|total| instances.len() as u64 >= total) {
            break;
        }
    }

    Ok(instances)
}

/// The `InstanceId`s of a `DescribeInstances` response.
fn instance_ids(response: &Value) -> Vec<String> {
    response
        .get("InstanceSet")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|instance| instance.get("InstanceId").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped as the CVM DescribeInstances reference's example, cut to what is read.
    #[test]
    fn discovered_instances_are_their_ids() {
        let response = json!({
            "TotalCount": 2,
            "InstanceSet": [
                {"InstanceId": "ins-xlsyru2j", "InstanceName": "a", "InstanceState": "RUNNING"},
                {"InstanceId": "ins-1vu7fz3o", "InstanceName": "b", "InstanceState": "STOPPED"}
            ],
            "RequestId": "r"
        });

        assert_eq!(instance_ids(&response), ["ins-xlsyru2j", "ins-1vu7fz3o"]);
        assert!(instance_ids(&json!({"TotalCount": 0})).is_empty());
    }
}
