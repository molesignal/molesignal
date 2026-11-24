// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The settings of a CloudMonitor connector, read from its `config_json`.

use kernel::{Error, Result};
use serde_json::Value;

use crate::infra::connectors::{
    cloud_metrics::{Schedule, list},
    config::text,
};

const KIND: &str = "aliyun_cms_metrics";
const DEFAULT_PERIOD_SECS: i64 = 300;
const DEFAULT_DELAY_SECS: i64 = 300;
const DEFAULT_STATISTICS: [&str; 2] = ["Average", "Maximum"];

/// How many metrics one connector reads: each costs at least one request a pull.
const MAX_METRICS: usize = 100;

/// How many instances one `Dimensions` filter may name, as CloudMonitor allows.
const MAX_DIMENSION_INSTANCES: usize = 50;

#[derive(Debug, Clone)]
pub(super) struct Settings {
    pub access_key: String,
    pub secret_key: String,
    pub region: String,
    pub namespace: String,
    pub metrics: Vec<String>,
    pub statistics: Vec<String>,
    /// The `Dimensions` filter as CloudMonitor takes it, a JSON text; `None` reads every instance
    /// of the account.
    pub dimensions: Option<String>,
    pub schedule: Schedule,
}

impl Settings {
    pub fn parse(config: &Value) -> Result<Self> {
        let access_key = needed(config, "accessKey", "access_key")?;
        let secret_key = needed(config, "secretKey", "secret_key")?;
        let region = needed(config, "region", "region")?;
        let namespace = needed(config, "namespace", "namespace")?;

        if !region
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        {
            return Err(Error::invalid(format!(
                "`region` must be a region ID such as cn-hangzhou, not `{region}`"
            )));
        }

        let metrics = list(config, "metrics");

        if metrics.is_empty() {
            return Err(Error::invalid(format!(
                "`metrics` is required for {KIND}: name at least one metric, such as CPUUtilization"
            )));
        }

        if metrics.len() > MAX_METRICS {
            return Err(Error::invalid(format!(
                "`metrics` names {} metrics; one connector reads at most {MAX_METRICS}",
                metrics.len()
            )));
        }

        let mut statistics = list(config, "statistics");

        if statistics.is_empty() {
            statistics = DEFAULT_STATISTICS.map(str::to_string).to_vec();
        }

        Ok(Self {
            access_key,
            secret_key,
            region,
            namespace,
            metrics,
            statistics,
            dimensions: dimensions(config)?,
            schedule: Schedule::parse(config, DEFAULT_PERIOD_SECS, DEFAULT_DELAY_SECS)?,
        })
    }

    /// The CloudMonitor endpoint of the region: every region has `metrics.<region>.aliyuncs.com`.
    pub fn host(&self) -> String {
        format!("metrics.{}.aliyuncs.com", self.region)
    }

    /// The namespace as a part of a metric name: `acs_ecs_dashboard` is `ecs_dashboard`.
    pub fn short_namespace(&self) -> &str {
        self.namespace
            .strip_prefix("acs_")
            .unwrap_or(&self.namespace)
    }
}

/// A required setting, or an error naming it the way the form shows it.
fn needed(config: &Value, key: &str, shown: &str) -> Result<String> {
    match text(config, key) {
        "" => Err(Error::invalid(format!("`{shown}` is required for {KIND}"))),
        value => Ok(value.to_string()),
    }
}

/// `dimensions`: a JSON object or array, given as JSON or as its text.
fn dimensions(config: &Value) -> Result<Option<String>> {
    let invalid = || {
        Error::invalid(
            "`dimensions` must be a JSON object or array, such as {\"instanceId\":\"i-abc\"}",
        )
    };

    let value = match config.get("dimensions") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(_)) if text(config, "dimensions").is_empty() => return Ok(None),
        Some(Value::String(_)) => {
            serde_json::from_str::<Value>(text(config, "dimensions")).map_err(|_| invalid())?
        }
        Some(value) => value.clone(),
    };

    match &value {
        Value::Object(fields) if fields.is_empty() => Ok(None),
        Value::Array(items) if items.is_empty() => Ok(None),
        Value::Array(items) if items.len() > MAX_DIMENSION_INSTANCES => {
            Err(Error::invalid(format!(
                "`dimensions` names {} instances; CloudMonitor takes at most \
                 {MAX_DIMENSION_INSTANCES} in one query",
                items.len()
            )))
        }
        Value::Object(_) | Value::Array(_) => Ok(Some(value.to_string())),
        _ => Err(invalid()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn config() -> Value {
        json!({
            "accessKey": "LTAI-example",
            "secretKey": "secret",
            "region": "cn-hangzhou",
            "namespace": "acs_ecs_dashboard",
            "metrics": "CPUUtilization, memory_usedutilization",
        })
    }

    #[test]
    fn the_defaults_fill_what_is_left_out() {
        let settings = Settings::parse(&config()).unwrap();

        assert_eq!(
            settings.metrics,
            ["CPUUtilization", "memory_usedutilization"]
        );
        assert_eq!(settings.statistics, ["Average", "Maximum"]);
        assert_eq!(settings.schedule.period_secs, 300);
        assert_eq!(settings.schedule.delay_secs, 300);
        assert_eq!(settings.dimensions, None);
        assert_eq!(settings.host(), "metrics.cn-hangzhou.aliyuncs.com");
        assert_eq!(settings.short_namespace(), "ecs_dashboard");
    }

    #[test]
    fn missing_settings_are_named_in_snake_case() {
        for key in ["accessKey", "secretKey", "region", "namespace", "metrics"] {
            let mut config = config();
            config.as_object_mut().unwrap().remove(key);

            let error = Settings::parse(&config).unwrap_err().to_string();
            let shown = match key {
                "accessKey" => "access_key",
                "secretKey" => "secret_key",
                other => other,
            };

            assert!(error.contains(&format!("`{shown}`")), "{error}");
        }
    }

    #[test]
    fn a_region_is_an_id_and_a_period_whole_minutes() {
        let mut bad = config();
        bad["region"] = json!("cn-hangzhou.evil.com/");
        assert!(Settings::parse(&bad).is_err());

        let mut bad = config();
        bad["period"] = json!(90);
        assert!(Settings::parse(&bad).is_err());

        let mut good = config();
        good["period"] = json!("60");
        assert_eq!(Settings::parse(&good).unwrap().schedule.period_secs, 60);
    }

    #[test]
    fn dimensions_pass_through_as_json_text() {
        let mut given = config();
        given["dimensions"] = json!("{\"instanceId\":\"i-abc\"}");
        assert_eq!(
            Settings::parse(&given).unwrap().dimensions.as_deref(),
            Some("{\"instanceId\":\"i-abc\"}")
        );

        given["dimensions"] = json!([{"instanceId": "i-abc"}]);
        assert_eq!(
            Settings::parse(&given).unwrap().dimensions.as_deref(),
            Some("[{\"instanceId\":\"i-abc\"}]")
        );

        given["dimensions"] = json!("  ");
        assert_eq!(Settings::parse(&given).unwrap().dimensions, None);

        given["dimensions"] = json!("instanceId=i-abc");
        assert!(Settings::parse(&given).is_err());

        given["dimensions"] = json!(
            (0..51)
                .map(|i| json!({"instanceId": i}))
                .collect::<Vec<_>>()
        );
        assert!(Settings::parse(&given).is_err());
    }
}
