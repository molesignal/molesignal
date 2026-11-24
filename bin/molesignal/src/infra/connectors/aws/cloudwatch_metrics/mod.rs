// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Pulling CloudWatch metrics with `GetMetricData`, for what a metric stream does not carry: a
//! namespace without a stream, metrics reported less often than every two hours, an account that
//! has no Firehose reachable from it.
//!
//! Each pull lists the series of the namespace that had data in the last three hours (all its
//! metrics, or those `metrics` names), then asks for every configured statistic of every series
//! over the next window of whole periods (see [`cloud_metrics::window`]), 500 queries a request.
//! Samples are named like those of a metric stream: `aws_ec2_cpuutilization_average`, with
//! `region` and `dimension_<Name>` labels.
//!
//! Settings (`config_json`): `region`, `namespace`, `metrics` (optional), `statistics` (default
//! `Average`, `Maximum`), `period` (default 300 s), `delay` (default 600 s: CloudWatch completes
//! an aggregate some minutes after its period), and the AWS credentials of [`AwsAuth`]. Each query
//! of a statistic of a series is billed as one metric requested.

use async_trait::async_trait;
use kernel::{Error, Result, time::TimestampMicros};
use object_store::aws::AwsCredentialProvider;
use serde_json::Value;

use super::{
    CredentialSource, credential_provider, kinesis_firehose::metric_streams::statistic_suffix,
};
use crate::infra::connectors::{
    ConnectorKind, PullBatch, PullConnector, PullContext,
    cloud_metrics::{self, MetricPoint, Schedule, Window, naming},
    config::{aws::AwsAuth, text},
    repo::Connector,
};

mod api;

use api::{Api, DataRequest, Query, Series};

const KIND: ConnectorKind = ConnectorKind::AwsCloudwatchMetrics;
const DEFAULT_PERIOD_SECS: i64 = 300;
const DEFAULT_DELAY_SECS: i64 = 600;
const DEFAULT_STATISTICS: [&str; 2] = ["Average", "Maximum"];

/// How many series one pull reads at most; the rest are left out with a warning.
const MAX_SERIES: usize = 2_000;

/// How many pages of `ListMetrics` one pull reads at most.
const MAX_LIST_PAGES: usize = 20;

/// Rejects settings that could never be pulled.
pub fn validate(config: &Value) -> Result<()> {
    cloud_metrics::required(config, "region", KIND)?;
    cloud_metrics::required(config, "namespace", KIND)?;
    statistics(config)?;
    Schedule::parse(config, DEFAULT_PERIOD_SECS, DEFAULT_DELAY_SECS)?;
    AwsAuth::parse(config)?;
    Ok(())
}

/// The statistics to ask for: the standard five and percentiles such as `p99` or `p99.9`.
fn statistics(config: &Value) -> Result<Vec<String>> {
    let named = cloud_metrics::list(config, "statistics");

    let named = if named.is_empty() {
        DEFAULT_STATISTICS.iter().map(|s| s.to_string()).collect()
    } else {
        named
    };

    for statistic in &named {
        let standard = matches!(
            statistic.as_str(),
            "Average" | "Sum" | "Minimum" | "Maximum" | "SampleCount"
        );
        let percentile = statistic
            .strip_prefix('p')
            .is_some_and(|rest| rest.parse::<f64>().is_ok_and(|p| p > 0.0 && p < 100.0));

        if !standard && !percentile {
            return Err(Error::invalid(format!(
                "`statistics` takes Average, Sum, Minimum, Maximum, SampleCount and percentiles \
                 such as p99, not `{statistic}`"
            )));
        }
    }

    Ok(named)
}

pub struct AwsCloudwatchMetrics {
    region: String,
    namespace: String,
    metrics: Vec<String>,
    /// The settings, or why they cannot be pulled with.
    settings: std::result::Result<(Schedule, Vec<String>), String>,
    credentials: std::result::Result<AwsCredentialProvider, String>,
}

impl AwsCloudwatchMetrics {
    pub fn new(connector: &Connector, source: CredentialSource) -> Self {
        let config = &connector.config_json;
        let region = text(config, "region").to_string();

        let settings = Schedule::parse(config, DEFAULT_PERIOD_SECS, DEFAULT_DELAY_SECS)
            .and_then(|schedule| Ok((schedule, statistics(config)?)))
            .map_err(|e| e.to_string());

        let credentials = AwsAuth::parse(config)
            .and_then(|auth| credential_provider(connector, &auth, &region, source))
            .map_err(|e| e.to_string());

        Self {
            region,
            namespace: text(config, "namespace").to_string(),
            metrics: cloud_metrics::list(config, "metrics"),
            settings,
            credentials,
        }
    }

    fn api(&self) -> Result<Api<'_>> {
        if self.region.is_empty() || self.namespace.is_empty() {
            return Err(Error::invalid("`region` and `namespace` are required"));
        }

        let credentials = self
            .credentials
            .as_ref()
            .map_err(|reason| Error::invalid(reason.clone()))?;

        Ok(Api {
            region: &self.region,
            credentials,
            client: cloud_metrics::http_client()?,
        })
    }

    /// One page of `ListMetrics`: it takes the credentials and the `cloudwatch:ListMetrics`
    /// permission a pull takes, and moves nothing.
    pub async fn check(&self) -> Result<()> {
        let api = self.api()?;

        api.list_metrics(
            &self.namespace,
            self.metrics.first().map(String::as_str),
            None,
        )
        .await?;

        Ok(())
    }

    /// The series to read, at most [`MAX_SERIES`]; whether some were left out.
    async fn series(&self, api: &Api<'_>) -> Result<(Vec<Series>, bool)> {
        let names: Vec<Option<&str>> = if self.metrics.is_empty() {
            vec![None]
        } else {
            self.metrics
                .iter()
                .map(|name| Some(name.as_str()))
                .collect()
        };

        let mut series = Vec::new();

        for name in names {
            let mut token: Option<String> = None;

            for _ in 0..MAX_LIST_PAGES {
                let (page, next) = api
                    .list_metrics(&self.namespace, name, token.as_deref())
                    .await?;

                series.extend(page);

                if series.len() >= MAX_SERIES {
                    series.truncate(MAX_SERIES);
                    return Ok((series, true));
                }

                match next {
                    Some(next) => token = Some(next),
                    None => break,
                }
            }
        }

        Ok((series, false))
    }

    fn point(&self, series: &Series, statistic: &str, timestamp: i64, value: f64) -> MetricPoint {
        let namespace = self
            .namespace
            .strip_prefix("AWS/")
            .unwrap_or(&self.namespace);

        let mut labels = vec![("region".to_string(), self.region.clone())];

        labels.extend(
            series
                .dimensions
                .iter()
                .map(|(name, value)| (naming::dimension_label(name), value.clone())),
        );

        MetricPoint {
            name: naming::metric_name(
                "aws",
                namespace,
                &series.metric_name,
                Some(&statistic_name(statistic)),
            ),
            timestamp: cloud_metrics::at_seconds(timestamp),
            value,
            labels,
            unit: None,
        }
    }
}

/// The exported suffix of a statistic: `SampleCount` → `sample_count`, `p99.9` → `p99_9`.
fn statistic_name(statistic: &str) -> String {
    match statistic {
        "SampleCount" => "sample_count".to_string(),
        other => statistic_suffix(other),
    }
}

#[async_trait]
impl PullConnector for AwsCloudwatchMetrics {
    async fn run_once(&self, ctx: &PullContext) -> Result<PullBatch> {
        let (schedule, statistics) = self
            .settings
            .as_ref()
            .map_err(|reason| Error::invalid(reason.clone()))?;

        let now = TimestampMicros::now().0 / 1_000_000;

        let Some(window) = Window::next(*schedule, &ctx.position, now) else {
            return Ok(PullBatch::default());
        };

        let api = self.api()?;
        let (series, capped) = self.series(&api).await?;

        let queries: Vec<Query> = (0..series.len())
            .flat_map(|index| {
                statistics.iter().map(move |statistic| Query {
                    series: index,
                    statistic: statistic.clone(),
                })
            })
            .collect();

        let mut points = Vec::new();

        for chunk in queries.chunks(api::MAX_QUERIES) {
            let request = DataRequest {
                namespace: &self.namespace,
                period_secs: schedule.period_secs,
                series: &series,
                queries: chunk,
                start: window.start,
                end: window.end,
            };

            let mut token: Option<String> = None;

            loop {
                let (results, next) = api.get_metric_data(&request, token.as_deref()).await?;

                for result in results {
                    let Some(query) = result
                        .id
                        .strip_prefix('m')
                        .and_then(|index| index.parse::<usize>().ok())
                        .and_then(|index| chunk.get(index))
                    else {
                        continue;
                    };

                    for (at, value) in result.timestamps.iter().zip(&result.values) {
                        points.push(self.point(
                            &series[query.series],
                            &query.statistic,
                            *at,
                            *value,
                        ));
                    }
                }

                match next {
                    Some(next) => token = Some(next),
                    None => break,
                }
            }
        }

        let warning = window.warning().or_else(|| {
            capped.then(|| {
                format!("the namespace has more than {MAX_SERIES} series: only the first {MAX_SERIES} were read; name `metrics` to choose")
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

#[cfg(test)]
mod tests {
    use domain::metrics::METRIC_NAME_FIELD;
    use serde_json::json;

    use super::*;

    #[test]
    fn settings_need_a_region_a_namespace_and_known_statistics() {
        let ok = json!({"region": "us-east-1", "namespace": "AWS/EC2", "auth": "node_role"});

        assert!(validate(&ok).is_ok());
        assert!(validate(&json!({"namespace": "AWS/EC2", "auth": "node_role"})).is_err());
        assert!(validate(&json!({"region": "us-east-1", "auth": "node_role"})).is_err());

        let mut odd = ok.clone();
        odd["statistics"] = json!("Average, Median");

        assert!(validate(&odd).is_err());

        let mut percentiles = ok.clone();
        percentiles["statistics"] = json!(["p99", "p99.9", "SampleCount"]);

        assert!(validate(&percentiles).is_ok());
    }

    #[test]
    fn statistics_are_exported_by_their_snake_case_names() {
        assert_eq!(statistic_name("Average"), "average");
        assert_eq!(statistic_name("SampleCount"), "sample_count");
        assert_eq!(statistic_name("p99.9"), "p99_9");
    }

    #[test]
    fn a_value_is_named_like_a_metric_stream_sample() {
        let connector = Connector {
            id: kernel::ids::Id::new(),
            org_id: kernel::ids::Id::new(),
            name: "c".into(),
            kind: KIND.as_str().into(),
            config_json: json!({"region": "eu-west-1", "namespace": "AWS/EC2", "auth": "node_role"}),
            enabled: true,
            last_run_at: None,
            read_state: Value::Null,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        };

        let source = AwsCloudwatchMetrics::new(&connector, CredentialSource::Fresh);
        let series = Series {
            metric_name: "NetworkIn".into(),
            dimensions: vec![("InstanceId".into(), "i-0abc".into())],
        };

        let event = source
            .point(&series, "Sum", 1_700_000_100, 42.0)
            .into_event()
            .unwrap();

        assert_eq!(event.fields[METRIC_NAME_FIELD], "aws_ec2_network_in_sum");
        assert_eq!(event.fields["dimension_InstanceId"], "i-0abc");
        assert_eq!(event.fields["region"], "eu-west-1");
        assert_eq!(event.timestamp.0, 1_700_000_100_000_000);
    }
}
