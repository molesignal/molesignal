// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Pulling Google Cloud Monitoring metrics.
//!
//! Each pull reads every configured metric type of one project over the next window of whole
//! periods (see [`cloud_metrics::window`]) with `timeSeries.list`, each series aligned to the
//! period as its descriptor allows ([`series::plan`]). Samples are named
//! `gcp_<service>_<path>_<statistic>`, `compute.googleapis.com/instance/cpu/utilization` as
//! `gcp_compute_instance_cpu_utilization_mean`, with `project_id`, `resource_type` and every
//! resource and metric label of the series.
//!
//! Settings (`config_json`): the secret `serviceAccountKey` (the JSON key file of a service
//! account with `roles/monitoring.viewer`), `projectId` (default the key's project), `metrics`
//! (metric types), an optional `filter` clause ANDed to each metric's filter, `period` (default
//! 300 s) and `delay` (default 300 s: Google documents that many metrics show up to 240 s late).

use std::{sync::LazyLock, time::Duration};

use async_trait::async_trait;
use kernel::{Error, Result, time::TimestampMicros};
use moka::future::Cache;
use serde_json::Value;
use signals::http_trace::{self, HttpTarget};

use crate::infra::connectors::{
    PullBatch, PullConnector, PullContext,
    cloud_metrics::{self, MetricPoint, Window},
    repo::Connector,
};

mod series;
mod settings;

use series::{Descriptor, Reader};
use settings::Settings;

use crate::infra::connectors::gcp::service_account::TokenSource;

/// Read access to Cloud Monitoring, nothing more.
const SCOPE: &str = "https://www.googleapis.com/auth/monitoring.read";

/// How many pages of one metric type one pull reads at most; the rest is left out with a warning.
const MAX_PAGES_PER_METRIC: usize = 20;

const API: &str = "google cloud monitoring";

/// Descriptors by service account, project and metric type. They hardly ever change, and asking
/// for each on every pull would double the requests.
static DESCRIPTORS: LazyLock<Cache<String, Descriptor>> = LazyLock::new(|| {
    Cache::builder()
        .max_capacity(4096)
        .time_to_live(Duration::from_secs(3600))
        .build()
});

/// Rejects settings that could never be pulled.
pub fn validate(config: &Value) -> Result<()> {
    Settings::parse(config).map(|_| ())
}

pub struct GcpMonitoringMetrics {
    /// The settings, or why they cannot be pulled with.
    settings: std::result::Result<Settings, String>,
}

impl GcpMonitoringMetrics {
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

    /// The descriptor of the first metric type: it takes a token for the key and the
    /// `monitoring.metricDescriptors.get` permission on the project, finds out whether the
    /// metric type is there, and moves nothing.
    pub async fn check(&self) -> Result<()> {
        let settings = self.settings()?;
        let http = cloud_metrics::http_client()?;
        let tokens = TokenSource::new(&settings.account, SCOPE);

        let Some(metric_type) = settings.metrics.first() else {
            return Err(Error::invalid(
                "`metrics` must name at least one metric type",
            ));
        };

        get(
            &http,
            &tokens,
            &series::descriptor_url(&settings.project_id, metric_type),
        )
        .await?;

        Ok(())
    }

    /// The descriptor of `metric_type`, kept for an hour.
    async fn descriptor(
        &self,
        http: &reqwest::Client,
        tokens: &TokenSource<'_>,
        settings: &Settings,
        metric_type: &str,
    ) -> Result<Descriptor> {
        let key = format!(
            "{}:{}:{metric_type}",
            settings.account.client_email, settings.project_id
        );

        if let Some(descriptor) = DESCRIPTORS.get(&key).await {
            return Ok(descriptor);
        }

        let url = series::descriptor_url(&settings.project_id, metric_type);
        let descriptor = Descriptor::parse(&get(http, tokens, &url).await?);

        DESCRIPTORS.insert(key, descriptor.clone()).await;
        Ok(descriptor)
    }

    /// The samples of `metric_type` over `window`, or why it was left out in part or in whole.
    async fn metric(
        &self,
        http: &reqwest::Client,
        tokens: &TokenSource<'_>,
        settings: &Settings,
        metric_type: &str,
        window: Window,
    ) -> Result<(Vec<MetricPoint>, Option<String>)> {
        let descriptor = match self.descriptor(http, tokens, settings, metric_type).await {
            Ok(descriptor) => descriptor,
            // A custom metric nobody has written yet has no descriptor; the others still count.
            Err(Error::NotFound(_)) => {
                return Ok((
                    Vec::new(),
                    Some(format!(
                        "`{metric_type}` is not a metric type of project {}",
                        settings.project_id
                    )),
                ));
            }
            Err(e) => return Err(e),
        };

        let plan = match series::plan(&descriptor) {
            Ok(plan) => plan,
            Err(why) => {
                return Ok((
                    Vec::new(),
                    Some(format!("`{metric_type}` is left out: {why}")),
                ));
            }
        };

        let reader = Reader {
            settings,
            metric_type,
            plan,
            descriptor: &descriptor,
            window,
        };

        let mut points = Vec::new();
        let mut page_token: Option<String> = None;

        for _ in 0..MAX_PAGES_PER_METRIC {
            let url = series::list_url(settings, metric_type, plan, window, page_token.as_deref());
            let (page, next) = reader.page(&get(http, tokens, &url).await?);

            points.extend(page);
            page_token = next;

            if page_token.is_none() {
                return Ok((points, None));
            }
        }

        let warning = format!(
            "`{metric_type}` has more than {} points a pull: the rest were left out; narrow it \
             with `filter`",
            MAX_PAGES_PER_METRIC as u32 * series::PAGE_SIZE
        );

        Ok((points, Some(warning)))
    }
}

#[async_trait]
impl PullConnector for GcpMonitoringMetrics {
    async fn run_once(&self, ctx: &PullContext) -> Result<PullBatch> {
        let settings = self.settings()?;
        let now = TimestampMicros::now().0 / 1_000_000;

        let Some(window) = Window::next(settings.schedule, &ctx.position, now) else {
            return Ok(PullBatch::default());
        };

        let http = cloud_metrics::http_client()?;
        let tokens = TokenSource::new(&settings.account, SCOPE);
        let mut points = Vec::new();
        let mut warnings: Vec<String> = window.warning().into_iter().collect();

        // One metric type at a time: the requests a pull makes stay few and in order.
        for metric_type in &settings.metrics {
            let (read, warning) = self
                .metric(&http, &tokens, settings, metric_type, window)
                .await?;

            points.extend(read);
            warnings.extend(warning);
        }

        Ok(PullBatch {
            metrics: cloud_metrics::events(points),
            position: Some(window.position()),
            more: window.more,
            warning: (!warnings.is_empty()).then(|| warnings.join("; ")),
            ..PullBatch::default()
        })
    }
}

/// A GET with a bearer token, its JSON answer, or the refusal as an error. A refused token is
/// dropped so that the next pull signs in again.
async fn get(http: &reqwest::Client, tokens: &TokenSource<'_>, url: &str) -> Result<Value> {
    let token = tokens.token(http).await?;
    let request = http.get(url).bearer_auth(token);

    let response = http_trace::send(http, request, HttpTarget::ThirdParty)
        .await
        .map_err(|e| Error::unavailable(format!("{API} request: {e}")))?;

    let status = response.status().as_u16();

    let text = response
        .text()
        .await
        .map_err(|e| Error::unavailable(format!("{API} response: {e}")))?;

    if !(200..300).contains(&status) {
        if status == 401 {
            tokens.forget().await;
        }

        return Err(cloud_metrics::refusal(API, status, &reason(&text)));
    }

    serde_json::from_str(&text)
        .map_err(|e| Error::unavailable(format!("{API} answered with something not JSON: {e}")))
}

/// The status and message of a Google error answer (`{"error": {"status", "message"}}`), or the
/// answer itself.
fn reason(text: &str) -> String {
    let error = serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|body| body.get("error").cloned());

    let field = |key: &str| {
        error
            .as_ref()
            .and_then(|error| error.get(key))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };

    match (field("status"), field("message")) {
        (status, message) if !status.is_empty() || !message.is_empty() => {
            format!("{status} {message}").trim().to_string()
        }
        _ => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_answer_reads_as_its_status_and_message() {
        let text = r#"{"error":{"code":403,"message":"Permission monitoring.timeSeries.list denied (or the resource may not exist).","status":"PERMISSION_DENIED"}}"#;

        assert_eq!(
            reason(text),
            "PERMISSION_DENIED Permission monitoring.timeSeries.list denied (or the resource may \
             not exist)."
        );
        assert_eq!(reason("<html>502</html>"), "<html>502</html>");
    }
}
