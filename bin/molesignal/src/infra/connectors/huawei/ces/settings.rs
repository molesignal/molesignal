// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The settings of a Huawei Cloud Eye connector, read from its `config_json`.

use kernel::{Error, Result};
use serde_json::Value;

use crate::infra::connectors::{
    cloud_metrics::{Schedule, list},
    config::text,
};

const KIND: &str = "huawei_ces_metrics";
const DEFAULT_PERIOD_SECS: i64 = 300;
const DEFAULT_DELAY_SECS: i64 = 300;

/// The aggregation periods Cloud Eye keeps, in seconds (raw data, `1`, is not offered).
const PERIODS: [i64; 5] = [300, 1_200, 3_600, 14_400, 86_400];

/// The statistics Cloud Eye aggregates, as `filter` names them.
const STATISTICS: [&str; 5] = ["average", "max", "min", "sum", "variance"];

/// How many metrics one connector reads.
const MAX_METRICS: usize = 100;

#[derive(Debug, Clone)]
pub(super) struct Settings {
    pub access_key: String,
    pub secret_key: String,
    pub region: String,
    pub project_id: String,
    pub namespace: String,
    pub metrics: Vec<String>,
    /// Each one of [`STATISTICS`], no repeats.
    pub statistics: Vec<String>,
    pub schedule: Schedule,
}

impl Settings {
    pub fn parse(config: &Value) -> Result<Self> {
        let access_key = needed(config, "accessKey", "access_key")?;
        let secret_key = needed(config, "secretKey", "secret_key")?;
        let region = needed(config, "region", "region")?;
        let project_id = needed(config, "projectId", "project_id")?;
        let namespace = needed(config, "namespace", "namespace")?;

        if !region
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        {
            return Err(Error::invalid(format!(
                "`region` must be a region ID such as cn-north-4, not `{region}`"
            )));
        }

        if !project_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        {
            return Err(Error::invalid(
                "`project_id` must be the project ID of the region, letters and digits",
            ));
        }

        let metrics = list(config, "metrics");

        if metrics.is_empty() {
            return Err(Error::invalid(format!(
                "`metrics` is required for {KIND}: name at least one metric, such as cpu_util"
            )));
        }

        if metrics.len() > MAX_METRICS {
            return Err(Error::invalid(format!(
                "`metrics` names {} metrics; one connector reads at most {MAX_METRICS}",
                metrics.len()
            )));
        }

        let mut statistics: Vec<String> = Vec::new();

        for statistic in list(config, "statistics") {
            let statistic = statistic.to_ascii_lowercase();

            if !STATISTICS.contains(&statistic.as_str()) {
                return Err(Error::invalid(format!(
                    "`statistics` takes {}, not `{statistic}`",
                    STATISTICS.join(", ")
                )));
            }

            if !statistics.contains(&statistic) {
                statistics.push(statistic);
            }
        }

        if statistics.is_empty() {
            statistics.push("average".to_string());
        }

        let schedule = Schedule::parse(config, DEFAULT_PERIOD_SECS, DEFAULT_DELAY_SECS)?;

        if !PERIODS.contains(&schedule.period_secs) {
            return Err(Error::invalid(format!(
                "`period` must be one of {} seconds, the periods Cloud Eye aggregates",
                PERIODS.map(|period| period.to_string()).join(", ")
            )));
        }

        Ok(Self {
            access_key,
            secret_key,
            region,
            project_id,
            namespace,
            metrics,
            statistics,
            schedule,
        })
    }

    /// The Cloud Eye endpoint of the region.
    pub fn host(&self) -> String {
        format!("ces.{}.myhuaweicloud.com", self.region)
    }
}

/// A required setting, or an error naming it the way the form shows it.
fn needed(config: &Value, key: &str, shown: &str) -> Result<String> {
    match text(config, key) {
        "" => Err(Error::invalid(format!("`{shown}` is required for {KIND}"))),
        value => Ok(value.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn config() -> Value {
        json!({
            "accessKey": "AK",
            "secretKey": "SK",
            "region": "cn-north-4",
            "projectId": "0b2c3a4d5e6f7a8b9c0d1e2f3a4b5c6d",
            "namespace": "SYS.ECS",
            "metrics": "cpu_util\nmem_util",
        })
    }

    #[test]
    fn the_defaults_fill_what_is_left_out() {
        let settings = Settings::parse(&config()).unwrap();

        assert_eq!(settings.metrics, ["cpu_util", "mem_util"]);
        assert_eq!(settings.statistics, ["average"]);
        assert_eq!(settings.schedule.period_secs, 300);
        assert_eq!(settings.schedule.delay_secs, 300);
        assert_eq!(settings.host(), "ces.cn-north-4.myhuaweicloud.com");
    }

    #[test]
    fn only_the_aggregated_periods_are_taken() {
        let mut config = config();

        for (period, ok) in [
            (300, true),
            (1_200, true),
            (86_400, true),
            (60, false),
            (600, false),
        ] {
            config["period"] = json!(period);
            assert_eq!(Settings::parse(&config).is_ok(), ok, "{period}");
        }
    }

    #[test]
    fn statistics_are_the_filters_cloud_eye_knows() {
        let mut config = config();
        config["statistics"] = json!("Max, average, max");
        assert_eq!(
            Settings::parse(&config).unwrap().statistics,
            ["max", "average"]
        );

        config["statistics"] = json!("p95");
        assert!(Settings::parse(&config).is_err());
    }

    #[test]
    fn missing_or_unsafe_settings_are_refused() {
        let mut missing = config();
        missing.as_object_mut().unwrap().remove("projectId");

        let error = Settings::parse(&missing).unwrap_err().to_string();
        assert!(error.contains("`project_id`"), "{error}");

        let mut unsafe_id = config();
        unsafe_id["projectId"] = json!("../x");
        assert!(Settings::parse(&unsafe_id).is_err());
    }
}
