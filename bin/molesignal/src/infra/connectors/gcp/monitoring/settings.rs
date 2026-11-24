// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a Google Cloud Monitoring metrics connector's settings say, read and checked in one place
//! so that saving, checking and pulling accept the same thing.

use kernel::{Error, Result};
use serde_json::Value;

use crate::infra::connectors::{
    ConnectorKind,
    cloud_metrics::{self, Schedule},
    config::text,
    gcp::service_account::ServiceAccount,
};

const KIND: ConnectorKind = ConnectorKind::GcpMonitoringMetrics;
const DEFAULT_PERIOD_SECS: i64 = 300;

/// Google documents that many metrics become visible up to 240 seconds after they are sampled.
const DEFAULT_DELAY_SECS: i64 = 300;

/// How many metric types one connector may pull.
const MAX_METRICS: usize = 50;

/// The settings of one connector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Settings {
    pub account: ServiceAccount,
    /// The project whose metrics are read: `projectId`, or the key's own.
    pub project_id: String,
    /// Metric types such as `compute.googleapis.com/instance/cpu/utilization`.
    pub metrics: Vec<String>,
    /// A clause ANDed to every metric's filter.
    pub filter: Option<String>,
    pub schedule: Schedule,
}

impl Settings {
    pub(super) fn parse(config: &Value) -> Result<Self> {
        let account = ServiceAccount::parse(config, KIND)?;

        let project_id = match text(config, "projectId") {
            "" => account.project_id.trim().to_string(),
            project => project.to_string(),
        };

        if project_id.is_empty() {
            return Err(Error::invalid(
                "`project_id` is required when the service account key names no project",
            ));
        }

        // The project is put in a URL path: a stray `/` or `?` would send the request elsewhere.
        if !project_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | ':'))
        {
            return Err(Error::invalid(format!(
                "`project_id` must be a project ID such as my-project, not `{project_id}`"
            )));
        }

        Ok(Self {
            account,
            project_id,
            metrics: metrics(config)?,
            filter: Some(text(config, "filter"))
                .filter(|filter| !filter.is_empty())
                .map(str::to_string),
            schedule: Schedule::parse(config, DEFAULT_PERIOD_SECS, DEFAULT_DELAY_SECS)?,
        })
    }
}

fn metrics(config: &Value) -> Result<Vec<String>> {
    let metrics = cloud_metrics::list(config, "metrics");

    if metrics.is_empty() {
        return Err(Error::invalid(
            "`metrics` must name at least one metric type, such as \
             compute.googleapis.com/instance/cpu/utilization",
        ));
    }

    if metrics.len() > MAX_METRICS {
        return Err(Error::invalid(format!(
            "`metrics` names {} metric types: at most {MAX_METRICS} are pulled by one connector",
            metrics.len()
        )));
    }

    if let Some(bad) = metrics.iter().find(|metric| !is_metric_type(metric)) {
        return Err(Error::invalid(format!(
            "`metrics` takes metric types such as compute.googleapis.com/instance/cpu/utilization, \
             not `{bad}`"
        )));
    }

    Ok(metrics)
}

/// `<service>/<path>`, in the characters metric types use. The type is put in a URL path and in
/// a quoted filter, so nothing else may pass.
fn is_metric_type(value: &str) -> bool {
    let Some((service, path)) = value.split_once('/') else {
        return false;
    };

    !service.is_empty()
        && !path.is_empty()
        && !path.starts_with('/')
        && !path.ends_with('/')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '/' | ':'))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn key() -> Value {
        json!({
            "type": "service_account",
            "project_id": "my-project",
            "private_key_id": "abc",
            "private_key": crate::infra::connectors::gcp::service_account::test_key(),
            "client_email": "reader@my-project.iam.gserviceaccount.com",
            "client_id": "123",
            "token_uri": "https://oauth2.googleapis.com/token",
        })
    }

    fn config() -> Value {
        json!({
            "serviceAccountKey": key().to_string(),
            "metrics": ["compute.googleapis.com/instance/cpu/utilization"],
        })
    }

    fn with(key: &str, value: Value) -> Value {
        let mut config = config();
        config[key] = value;
        config
    }

    #[test]
    fn the_project_is_the_key_s_unless_one_is_named() {
        let settings = Settings::parse(&config()).unwrap();

        assert_eq!(settings.project_id, "my-project");
        assert_eq!(
            settings.account.client_email,
            "reader@my-project.iam.gserviceaccount.com"
        );
        assert_eq!(settings.schedule.period_secs, 300);
        assert_eq!(settings.schedule.delay_secs, 300);
        assert_eq!(settings.filter, None);

        let named = Settings::parse(&with("projectId", json!("other-project"))).unwrap();

        assert_eq!(named.project_id, "other-project");
    }

    #[test]
    fn the_key_may_be_an_object() {
        assert!(Settings::parse(&with("serviceAccountKey", key())).is_ok());
    }

    #[test]
    fn a_key_that_cannot_sign_in_with_google_is_refused() {
        let mut elsewhere = key();
        elsewhere["token_uri"] = json!("https://attacker.example/token");

        assert!(Settings::parse(&with("serviceAccountKey", elsewhere)).is_err());

        let mut broken = key();
        broken["private_key"] =
            json!("-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----");

        assert!(Settings::parse(&with("serviceAccountKey", broken)).is_err());
        assert!(Settings::parse(&with("serviceAccountKey", json!("not json"))).is_err());
        assert!(Settings::parse(&with("serviceAccountKey", json!(""))).is_err());
    }

    #[test]
    fn metric_types_and_the_project_are_checked() {
        assert!(Settings::parse(&with("metrics", json!([]))).is_err());
        assert!(Settings::parse(&with("metrics", json!("cpu"))).is_err());
        assert!(Settings::parse(&with("metrics", json!("a.googleapis.com/x\" OR 1"))).is_err());
        assert!(
            Settings::parse(&with(
                "metrics",
                json!("prometheus.googleapis.com/job:rate5m/gauge")
            ))
            .is_ok()
        );
        assert!(Settings::parse(&with("projectId", json!("p/../q"))).is_err());
        assert!(Settings::parse(&with("period", json!(90))).is_err());
    }
}
