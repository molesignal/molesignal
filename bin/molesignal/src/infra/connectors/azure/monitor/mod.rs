// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Pulling Azure Monitor platform metrics.
//!
//! Each pull lists the resources of one type in one region of one subscription with Azure
//! Resource Graph ([`resources`]), then asks the regional metrics endpoint for every configured
//! metric and aggregation of those resources over the next window of whole periods (see
//! [`cloud_metrics::window`]) with `metrics:getBatch`, 50 resources a request ([`batch`]). Samples
//! are named `azure_<type>_<metric>_<aggregation>`, `Percentage CPU` of
//! `Microsoft.Compute/virtualMachines` as `azure_compute_virtualmachines_percentage_cpu_average`,
//! with `subscription_id`, `resource_group`, `resource_name`, `resource_id`, `region`,
//! `dimension_<name>` and `tag_<key>` labels.
//!
//! Settings (`config_json`): the app registration that signs in (`tenantId`, `clientId`, the
//! secret `clientSecret`), what to read (`subscriptionId`, `region`, `resourceType`, `metrics`),
//! `aggregations` (default `average`), an optional dimension `filter` with the `top` series it
//! may split into per resource (default 100), `period` (default 300 s, one of Azure's time
//! grains) and `delay` (default 600 s). The app needs Monitoring Reader on the subscription:
//! `getBatch` is a subscription-level call.

use std::collections::HashMap;

use async_trait::async_trait;
use futures::{StreamExt, TryStreamExt, stream};
use kernel::{Error, Result, time::TimestampMicros};
use serde_json::{Value, json};
use signals::http_trace::{self, HttpTarget};

use crate::infra::connectors::{
    PullBatch, PullConnector, PullContext,
    cloud_metrics::{self, Window},
    repo::Connector,
};

mod batch;
mod resources;
mod settings;
mod token;

use resources::Resource;
use settings::Settings;
use token::{MANAGEMENT_SCOPE, METRICS_SCOPE, TokenSource};

/// How many resources one pull reads at most; the rest are left out with a warning.
const MAX_RESOURCES: usize = 2_000;

/// How many `getBatch` requests run at once.
const CONCURRENT_REQUESTS: usize = 4;

/// How many of the metrics Azure could not read a warning names.
const FAILURES_NAMED: usize = 3;

const RESOURCE_GRAPH: &str = "azure resource graph";
const METRICS_API: &str = "azure monitor metrics";

/// Rejects settings that could never be pulled.
pub fn validate(config: &Value) -> Result<()> {
    Settings::parse(config).map(|_| ())
}

pub struct AzureMonitorMetrics {
    /// The settings, or why they cannot be pulled with.
    settings: std::result::Result<Settings, String>,
}

impl AzureMonitorMetrics {
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

    /// One Resource Graph query for at most one resource: it takes the client credentials and
    /// read access to the subscription, and moves nothing.
    pub async fn check(&self) -> Result<()> {
        let settings = self.settings()?;
        let http = cloud_metrics::http_client()?;
        let tokens = TokenSource::new(settings, MANAGEMENT_SCOPE);
        let body = resources::request_body(settings, 1, None);

        post(
            &http,
            &tokens,
            resources::RESOURCE_GRAPH_URL,
            &body,
            RESOURCE_GRAPH,
        )
        .await?;

        Ok(())
    }

    /// The resources to read, by ID, and whether there were more than [`MAX_RESOURCES`].
    async fn resources(
        &self,
        http: &reqwest::Client,
        settings: &Settings,
    ) -> Result<(Vec<Resource>, bool)> {
        let tokens = TokenSource::new(settings, MANAGEMENT_SCOPE);
        let mut found = Vec::new();
        let mut skip_token: Option<String> = None;

        loop {
            let body =
                resources::request_body(settings, resources::PAGE_SIZE, skip_token.as_deref());
            let answer = post(
                http,
                &tokens,
                resources::RESOURCE_GRAPH_URL,
                &body,
                RESOURCE_GRAPH,
            )
            .await?;
            let page = resources::parse_page(&answer);

            found.extend(page.resources);
            skip_token = page.skip_token;

            if found.len() >= MAX_RESOURCES {
                let truncated = found.len() > MAX_RESOURCES || skip_token.is_some();

                found.truncate(MAX_RESOURCES);
                return Ok((found, truncated));
            }

            if skip_token.is_none() {
                return Ok((found, false));
            }
        }
    }

    /// The samples of every metric of `resources` over `window`.
    async fn metrics(
        &self,
        http: &reqwest::Client,
        settings: &Settings,
        resources: &[Resource],
        window: Window,
    ) -> Result<batch::Answer> {
        let tokens = TokenSource::new(settings, METRICS_SCOPE);

        let by_id: HashMap<String, Resource> = resources
            .iter()
            .map(|resource| (resource.id.to_ascii_lowercase(), resource.clone()))
            .collect();

        let mut requests = Vec::new();

        for batch_resources in resources.chunks(batch::MAX_RESOURCES_PER_CALL) {
            let ids: Vec<&str> = batch_resources
                .iter()
                .map(|resource| resource.id.as_str())
                .collect();

            for metrics in settings.metrics.chunks(batch::MAX_METRICS_PER_CALL) {
                requests.push((
                    batch::url(settings, metrics, window),
                    json!({ "resourceids": ids }),
                ));
            }
        }

        let tokens = &tokens;

        let bodies: Vec<Value> =
            stream::iter(requests)
                .map(move |(url, body)| async move {
                    post(http, tokens, &url, &body, METRICS_API).await
                })
                .buffer_unordered(CONCURRENT_REQUESTS)
                .try_collect()
                .await?;

        let mut answer = batch::Answer::default();

        for body in &bodies {
            let part = batch::parse(body, settings, &by_id, window);

            answer.points.extend(part.points);
            answer.failures.extend(part.failures);
        }

        Ok(answer)
    }
}

#[async_trait]
impl PullConnector for AzureMonitorMetrics {
    async fn run_once(&self, ctx: &PullContext) -> Result<PullBatch> {
        let settings = self.settings()?;
        let now = TimestampMicros::now().0 / 1_000_000;

        let Some(window) = Window::next(settings.schedule, &ctx.position, now) else {
            return Ok(PullBatch::default());
        };

        let http = cloud_metrics::http_client()?;
        let (resources, truncated) = self.resources(&http, settings).await?;
        let mut warnings: Vec<String> = window.warning().into_iter().collect();

        if truncated {
            warnings.push(format!(
                "there are more than {MAX_RESOURCES} {} resources in {}: only the first \
                 {MAX_RESOURCES} by ID were read",
                settings.resource_type, settings.region
            ));
        }

        let answer = if resources.is_empty() {
            warnings.push(format!(
                "subscription {} has no {} resources in {} that this app can read",
                settings.subscription_id, settings.resource_type, settings.region
            ));

            batch::Answer::default()
        } else {
            self.metrics(&http, settings, &resources, window).await?
        };

        if let Some(failed) = failures(&answer.failures) {
            warnings.push(failed);
        }

        Ok(PullBatch {
            metrics: cloud_metrics::events(answer.points),
            position: Some(window.position()),
            more: window.more,
            warning: (!warnings.is_empty()).then(|| warnings.join("; ")),
            ..PullBatch::default()
        })
    }
}

/// What a warning says about the metrics Azure could not read, `None` when it read them all.
fn failures(failures: &[String]) -> Option<String> {
    if failures.is_empty() {
        return None;
    }

    let mut named: Vec<&str> = Vec::new();

    for failure in failures {
        if named.len() < FAILURES_NAMED && !named.contains(&failure.as_str()) {
            named.push(failure);
        }
    }

    Some(format!(
        "Azure Monitor could not read {} metric series: {}",
        failures.len(),
        named.join("; ")
    ))
}

/// A POST of `body` with a bearer token for `tokens`'s scope, its JSON answer, or the refusal as
/// an error. A refused token is dropped so that the next pull signs in again.
async fn post(
    http: &reqwest::Client,
    tokens: &TokenSource<'_>,
    url: &str,
    body: &Value,
    api: &str,
) -> Result<Value> {
    let token = tokens.token(http).await?;
    let request = http.post(url).bearer_auth(token).json(body);

    let response = http_trace::send(http, request, HttpTarget::ThirdParty)
        .await
        .map_err(|e| Error::unavailable(format!("{api} request: {e}")))?;

    let status = response.status().as_u16();

    let text = response
        .text()
        .await
        .map_err(|e| Error::unavailable(format!("{api} response: {e}")))?;

    if !(200..300).contains(&status) {
        if status == 401 {
            tokens.forget().await;
        }

        return Err(cloud_metrics::refusal(api, status, &reason(&text)));
    }

    serde_json::from_str(&text)
        .map_err(|e| Error::unavailable(format!("{api} answered with something not JSON: {e}")))
}

/// The code and message of an Azure error answer (`{"error": {"code", "message"}}`), or the
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

    match (field("code"), field("message")) {
        (code, message) if !code.is_empty() || !message.is_empty() => {
            format!("{code} {message}").trim().to_string()
        }
        _ => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_answer_reads_as_its_code_and_message() {
        let text = r#"{"error":{"additionalInfo":[],"code":"BadRequest","message":"Detected invalid time range parameters"}}"#;

        assert_eq!(
            reason(text),
            "BadRequest Detected invalid time range parameters"
        );
        assert_eq!(reason("Service Unavailable"), "Service Unavailable");
    }

    #[test]
    fn a_warning_names_a_few_distinct_failures() {
        assert_eq!(failures(&[]), None);

        let many: Vec<String> = ["a", "a", "b", "c", "d"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        assert_eq!(
            failures(&many).unwrap(),
            "Azure Monitor could not read 5 metric series: a; b; c"
        );
    }
}
