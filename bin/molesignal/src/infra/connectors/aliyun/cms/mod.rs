// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Alibaba Cloud CloudMonitor metrics, pulled with `DescribeMetricList` (CMS API 2019-01-01).
//!
//! Each pull asks for every metric of `metrics` over the next [`Window`], one paged query per
//! metric. Without `dimensions` a query covers every instance of the account in the namespace, so
//! no instance list is kept. Requests go one after another, a little apart, well under the 50 a
//! second the account's query APIs share.
//!
//! Settings: `accessKey`, `secretKey` (an AccessKey pair with `cms:QueryMetricList`), `region`,
//! `namespace`, `metrics`, `statistics`, `dimensions`, `period`, `delay`; see [`settings`].

mod api;
mod data_points;
mod settings;

use std::time::Duration;

use async_trait::async_trait;
use kernel::{Error, Result, time::TimestampMicros};
use serde_json::Value;

use self::{
    api::{MAX_PAGE_LENGTH, Query},
    settings::Settings,
};
use crate::infra::connectors::{
    PullBatch, PullConnector, PullContext,
    cloud_metrics::{self, Window},
    repo::Connector,
};

/// How many requests one pull makes at most.
const MAX_REQUESTS_PER_PULL: usize = 300;

/// How many pages one metric reads in one pull at most.
const MAX_PAGES_PER_METRIC: usize = 50;

/// The pause between two requests.
const REQUEST_GAP: Duration = Duration::from_millis(25);

/// Rejects settings that could never work.
pub fn validate(config: &Value) -> Result<()> {
    Settings::parse(config).map(|_| ())
}

pub struct AliyunCmsMetrics {
    /// The settings; why they are unusable when they are.
    settings: std::result::Result<Settings, String>,
}

impl AliyunCmsMetrics {
    pub fn new(connector: &Connector) -> Self {
        Self {
            settings: Settings::parse(&connector.config_json).map_err(|e| e.to_string()),
        }
    }

    fn settings(&self) -> Result<&Settings> {
        self.settings
            .as_ref()
            .map_err(|reason| Error::invalid(reason.clone()))
    }

    /// One query of one record for the first metric over the last few periods: it takes the
    /// credentials and the permission a pull takes, and leaves the read position alone.
    pub async fn check(&self) -> Result<()> {
        let settings = self.settings()?;
        let client = cloud_metrics::http_client()?;
        let now_ms = TimestampMicros::now().0 / 1_000;
        let span_ms = settings.schedule.period_secs.max(300) * 3 * 1_000;

        api::describe_metric_list(
            &client,
            settings,
            &Query {
                metric: &settings.metrics[0],
                start_ms: now_ms - span_ms,
                end_ms: now_ms,
                length: 1,
                next_token: None,
            },
        )
        .await?;

        Ok(())
    }
}

#[async_trait]
impl PullConnector for AliyunCmsMetrics {
    async fn run_once(&self, ctx: &PullContext) -> Result<PullBatch> {
        let settings = self.settings()?;
        let now = TimestampMicros::now().0 / 1_000_000;

        let Some(window) = Window::next(settings.schedule, &ctx.position, now) else {
            return Ok(PullBatch::default());
        };

        let client = cloud_metrics::http_client()?;
        let mut points = Vec::new();
        let mut requests = 0;
        let mut cut_short = false;

        'metrics: for metric in &settings.metrics {
            let mut next_token: Option<String> = None;

            for _ in 0..MAX_PAGES_PER_METRIC {
                if requests >= MAX_REQUESTS_PER_PULL {
                    cut_short = true;
                    break 'metrics;
                }

                if requests > 0 {
                    tokio::time::sleep(REQUEST_GAP).await;
                }

                requests += 1;

                // CloudMonitor reads `(start, end]`; a millisecond back reads `[start, end)`.
                let page = api::describe_metric_list(
                    &client,
                    settings,
                    &Query {
                        metric,
                        start_ms: window.start * 1_000 - 1,
                        end_ms: window.end * 1_000 - 1,
                        length: MAX_PAGE_LENGTH,
                        next_token: next_token.as_deref(),
                    },
                )
                .await?;

                points.extend(data_points::points(
                    settings,
                    metric,
                    &page.datapoints,
                    &window,
                )?);

                next_token = page.next_token;

                if next_token.is_none() {
                    continue 'metrics;
                }
            }

            cut_short = true;
        }

        let warning = window.warning().or_else(|| {
            cut_short.then(|| {
                format!(
                    "a pull reads at most {MAX_REQUESTS_PER_PULL} pages and {MAX_PAGES_PER_METRIC} \
                     a metric: some series of this range were not read; name fewer metrics or \
                     narrow `dimensions`"
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
