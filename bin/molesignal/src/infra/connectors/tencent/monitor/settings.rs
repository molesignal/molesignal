// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The settings of a Tencent Cloud Monitor connector, read from its `config_json`.

use kernel::{Error, Result};
use serde_json::Value;

use crate::infra::connectors::{
    cloud_metrics::{Schedule, list},
    config::text,
};

const KIND: &str = "tencent_monitor_metrics";
const DEFAULT_PERIOD_SECS: i64 = 300;
const DEFAULT_DELAY_SECS: i64 = 300;

/// The namespace whose instances are found with CVM `DescribeInstances` when none are named.
pub(super) const CVM_NAMESPACE: &str = "QCE/CVM";

/// How many metrics one connector reads.
const MAX_METRICS: usize = 100;

/// How many instances one connector names.
const MAX_INSTANCES: usize = 2_000;

/// One instance: the dimensions that pick it out, such as `InstanceId=ins-abc`.
pub(super) type Instance = Vec<(String, String)>;

/// A statistic GetMonitorData returns, and its bit in `SpecifyStatistics`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Statistic {
    Avg,
    Max,
    Min,
}

impl Statistic {
    fn parse(text: &str) -> Result<Self> {
        match text.to_ascii_lowercase().as_str() {
            "avg" => Ok(Self::Avg),
            "max" => Ok(Self::Max),
            "min" => Ok(Self::Min),
            other => Err(Error::invalid(format!(
                "`statistics` takes avg, max and min, not `{other}`"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Avg => "avg",
            Self::Max => "max",
            Self::Min => "min",
        }
    }

    pub fn bit(self) -> u32 {
        match self {
            Self::Avg => 1,
            Self::Max => 2,
            Self::Min => 4,
        }
    }

    /// The field of a data point that holds this statistic's values.
    pub fn field(self) -> &'static str {
        match self {
            Self::Avg => "AvgValues",
            Self::Max => "MaxValues",
            Self::Min => "MinValues",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct Settings {
    pub secret_id: String,
    pub secret_key: String,
    pub region: String,
    pub namespace: String,
    pub metrics: Vec<String>,
    pub statistics: Vec<Statistic>,
    /// The instances to read; empty for `QCE/CVM` means every CVM instance of the region.
    pub instances: Vec<Instance>,
    pub schedule: Schedule,
}

impl Settings {
    pub fn parse(config: &Value) -> Result<Self> {
        let secret_id = needed(config, "accessKey", "access_key")?;
        let secret_key = needed(config, "secretKey", "secret_key")?;
        let region = needed(config, "region", "region")?;
        let namespace = needed(config, "namespace", "namespace")?;

        if !region
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        {
            return Err(Error::invalid(format!(
                "`region` must be a region ID such as ap-guangzhou, not `{region}`"
            )));
        }

        let metrics = list(config, "metrics");

        if metrics.is_empty() {
            return Err(Error::invalid(format!(
                "`metrics` is required for {KIND}: name at least one metric, such as CpuUsage"
            )));
        }

        if metrics.len() > MAX_METRICS {
            return Err(Error::invalid(format!(
                "`metrics` names {} metrics; one connector reads at most {MAX_METRICS}",
                metrics.len()
            )));
        }

        let mut statistics = Vec::new();

        for statistic in list(config, "statistics") {
            let statistic = Statistic::parse(&statistic)?;

            if !statistics.contains(&statistic) {
                statistics.push(statistic);
            }
        }

        if statistics.is_empty() {
            statistics.push(Statistic::Avg);
        }

        let instances = list(config, "instances")
            .iter()
            .map(|line| instance(line))
            .collect::<Result<Vec<_>>>()?;

        if instances.len() > MAX_INSTANCES {
            return Err(Error::invalid(format!(
                "`instances` names {} instances; one connector reads at most {MAX_INSTANCES}",
                instances.len()
            )));
        }

        if instances.is_empty() && namespace != CVM_NAMESPACE {
            return Err(Error::invalid(format!(
                "`instances` is required for {namespace}: Tencent Cloud Monitor reads named \
                 instances only, and only {CVM_NAMESPACE} instances are found by themselves"
            )));
        }

        Ok(Self {
            secret_id,
            secret_key,
            region,
            namespace,
            metrics,
            statistics,
            instances,
            schedule: Schedule::parse(config, DEFAULT_PERIOD_SECS, DEFAULT_DELAY_SECS)?,
        })
    }

    /// The namespace as a part of a metric name: `QCE/CVM` is `CVM`.
    pub fn short_namespace(&self) -> &str {
        self.namespace
            .strip_prefix("QCE/")
            .unwrap_or(&self.namespace)
    }

    /// `SpecifyStatistics`: the bits of every statistic to return.
    pub fn statistics_mask(&self) -> u32 {
        self.statistics
            .iter()
            .map(|statistic| statistic.bit())
            .sum()
    }
}

/// A required setting, or an error naming it the way the form shows it.
fn needed(config: &Value, key: &str, shown: &str) -> Result<String> {
    match text(config, key) {
        "" => Err(Error::invalid(format!("`{shown}` is required for {KIND}"))),
        value => Ok(value.to_string()),
    }
}

/// One line of `instances`: `Name=Value` pairs joined by `&`.
fn instance(line: &str) -> Result<Instance> {
    line.split('&')
        .map(str::trim)
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((name, value)) if !name.trim().is_empty() && !value.trim().is_empty() => {
                Ok((name.trim().to_string(), value.trim().to_string()))
            }
            _ => Err(Error::invalid(format!(
                "each line of `instances` is Name=Value pairs joined by &, such as \
                 InstanceId=ins-abc; `{line}` is not"
            ))),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn config() -> Value {
        json!({
            "accessKey": "AKIDexample",
            "secretKey": "secret",
            "region": "ap-guangzhou",
            "namespace": "QCE/CVM",
            "metrics": ["CpuUsage", "MemUsage"],
        })
    }

    #[test]
    fn the_defaults_fill_what_is_left_out() {
        let settings = Settings::parse(&config()).unwrap();

        assert_eq!(settings.statistics, [Statistic::Avg]);
        assert_eq!(settings.statistics_mask(), 1);
        assert!(settings.instances.is_empty());
        assert_eq!(settings.short_namespace(), "CVM");
        assert_eq!(settings.schedule.period_secs, 300);
        assert_eq!(settings.schedule.delay_secs, 300);
    }

    #[test]
    fn statistics_are_bits_of_one_mask() {
        let mut config = config();
        config["statistics"] = json!("max, avg, MIN, avg");

        let settings = Settings::parse(&config).unwrap();

        assert_eq!(
            settings.statistics,
            [Statistic::Max, Statistic::Avg, Statistic::Min]
        );
        assert_eq!(settings.statistics_mask(), 7);

        config["statistics"] = json!("p99");
        assert!(Settings::parse(&config).is_err());
    }

    #[test]
    fn an_instance_is_a_line_of_dimension_pairs() {
        assert_eq!(
            instance("InstanceId=ins-abc").unwrap(),
            [("InstanceId".to_string(), "ins-abc".to_string())]
        );
        assert_eq!(
            instance(" appid = 1250000000 & loadBalancerId=lb-x ").unwrap(),
            [
                ("appid".to_string(), "1250000000".to_string()),
                ("loadBalancerId".to_string(), "lb-x".to_string())
            ]
        );
        assert!(instance("ins-abc").is_err());
        assert!(instance("InstanceId=").is_err());

        let mut config = config();
        config["instances"] = json!("InstanceId=ins-1\nInstanceId=ins-2");
        assert_eq!(Settings::parse(&config).unwrap().instances.len(), 2);
    }

    #[test]
    fn a_namespace_other_than_cvm_names_its_instances() {
        let mut config = config();
        config["namespace"] = json!("QCE/CDB");

        let error = Settings::parse(&config).unwrap_err().to_string();
        assert!(error.contains("`instances`"), "{error}");

        config["instances"] = json!(["InstanceId=cdb-abc"]);
        assert!(Settings::parse(&config).is_ok());
    }

    #[test]
    fn missing_settings_are_named_in_snake_case() {
        let mut config = config();
        config.as_object_mut().unwrap().remove("secretKey");

        let error = Settings::parse(&config).unwrap_err().to_string();
        assert!(error.contains("`secret_key`"), "{error}");
    }
}
