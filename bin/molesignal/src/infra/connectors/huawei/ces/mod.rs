// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Huawei Cloud Eye metrics, pulled with the CES API v1.
//!
//! `BatchListMetricData` reads only series named with their dimensions, so a pull first lists the
//! series of each metric with `ListMetrics` (once while the connector lives), then reads the next
//! [`Window`] of them: one call set per statistic, since a call takes one `filter`, in batches that
//! keep series × periods within the 3000 points a call returns. Calls go one after another, apart
//! enough to stay under 500 a minute for the data and 300 a minute for the lists.
//!
//! Requests are signed with the AK/SK signature of Huawei Cloud's API Gateway ([`sign`]).
//!
//! Settings: `accessKey`, `secretKey` (AK/SK), `region`, `projectId`, `namespace`, `metrics`,
//! `statistics`, `period`, `delay`; see [`settings`].

mod api;
mod data_points;
mod settings;
mod sign;

use std::time::Duration;

use async_trait::async_trait;
use kernel::{Error, Result, time::TimestampMicros};
use serde_json::Value;
use tokio::sync::OnceCell;

use self::{
    api::{LIST_LIMIT, Series},
    settings::Settings,
};
use crate::infra::connectors::{
    PullBatch, PullConnector, PullContext,
    cloud_metrics::{self, Window},
    repo::Connector,
};

/// How many series one `BatchListMetricData` call names at most.
const MAX_SERIES_PER_CALL: usize = 500;

/// How many data points one `BatchListMetricData` call returns at most.
const MAX_POINTS_PER_CALL: usize = 3_000;

/// How many `ListMetrics` pages one metric lists at most.
const MAX_LIST_PAGES: usize = 10;

/// How many `BatchListMetricData` calls one pull makes at most.
const MAX_CALLS_PER_PULL: usize = 60;

/// The pause between two `BatchListMetricData` calls: under 500 a minute.
const DATA_GAP: Duration = Duration::from_millis(130);

/// The pause between two `ListMetrics` calls: under 300 a minute.
const LIST_GAP: Duration = Duration::from_millis(210);

/// Rejects settings that could never work.
pub fn validate(config: &Value) -> Result<()> {
    Settings::parse(config).map(|_| ())
}

pub struct HuaweiCesMetrics {
    /// The settings; why they are unusable when they are.
    settings: std::result::Result<Settings, String>,
    /// The series of every metric, listed once while the connector lives.
    series: OnceCell<Vec<Series>>,
}

impl HuaweiCesMetrics {
    pub fn new(connector: &Connector) -> Self {
        Self {
            settings: Settings::parse(&connector.config_json).map_err(|e| e.to_string()),
            series: OnceCell::new(),
        }
    }

    fn settings(&self) -> Result<&Settings> {
        self.settings
            .as_ref()
            .map_err(|reason| Error::invalid(reason.clone()))
    }

    /// One `ListMetrics` of one series of the first metric: it proves the credentials, the
    /// project and the permission, and leaves the read position alone.
    pub async fn check(&self) -> Result<()> {
        let settings = self.settings()?;
        let client = cloud_metrics::http_client()?;
        let metric = &settings.metrics[0];
        let page = api::list_metrics(&client, settings, metric, 1, None).await?;

        if page.series.is_empty() {
            return Err(Error::not_found(format!(
                "Cloud Eye has no series of `{metric}` in {} in this project",
                settings.namespace
            )));
        }

        Ok(())
    }

    async fn series(&self, client: &reqwest::Client) -> Result<&[Series]> {
        let settings = self.settings()?;

        let series = self
            .series
            .get_or_try_init(|| list_series(client, settings))
            .await?;

        Ok(series)
    }
}

#[async_trait]
impl PullConnector for HuaweiCesMetrics {
    async fn run_once(&self, ctx: &PullContext) -> Result<PullBatch> {
        let settings = self.settings()?;
        let now = TimestampMicros::now().0 / 1_000_000;

        let Some(window) = Window::next(settings.schedule, &ctx.position, now) else {
            return Ok(PullBatch::default());
        };

        let client = cloud_metrics::http_client()?;
        let series = self.series(&client).await?;
        let periods = window.periods(settings.schedule).count();
        let batch = series_per_call(periods);
        let mut points = Vec::new();
        let mut calls = 0;
        let mut cut_short = false;

        'statistics: for statistic in &settings.statistics {
            for chunk in series.chunks(batch) {
                if calls >= MAX_CALLS_PER_PULL {
                    cut_short = true;
                    break 'statistics;
                }

                if calls > 0 {
                    tokio::time::sleep(DATA_GAP).await;
                }

                calls += 1;

                let request =
                    api::batch_request(settings, chunk, statistic, window.start, window.end);

                let answer = api::batch_list_metric_data(&client, settings, &request).await?;

                points.extend(data_points::points(settings, statistic, &answer, &window));
            }
        }

        let warning = window.warning().or_else(|| {
            cut_short.then(|| {
                format!(
                    "a pull makes at most {MAX_CALLS_PER_PULL} BatchListMetricData calls: some \
                     series of this range were not read; name fewer metrics or statistics"
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

/// How many series one call can name for `periods` periods each.
fn series_per_call(periods: usize) -> usize {
    (MAX_POINTS_PER_CALL / periods.max(1)).clamp(1, MAX_SERIES_PER_CALL)
}

/// Every series of every metric of the settings, a page at a time.
async fn list_series(client: &reqwest::Client, settings: &Settings) -> Result<Vec<Series>> {
    let mut series = Vec::new();
    let mut calls = 0;

    for metric in &settings.metrics {
        let mut start: Option<String> = None;

        for _ in 0..MAX_LIST_PAGES {
            if calls > 0 {
                tokio::time::sleep(LIST_GAP).await;
            }

            calls += 1;

            let page =
                api::list_metrics(client, settings, metric, LIST_LIMIT, start.as_deref()).await?;

            series.extend(page.series);
            start = page.marker;

            if start.is_none() {
                break;
            }
        }
    }

    Ok(series)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_keeps_series_times_periods_within_the_limit() {
        assert_eq!(series_per_call(1), 500);
        assert_eq!(series_per_call(12), 250);
        assert_eq!(series_per_call(0), 500);
    }
}
