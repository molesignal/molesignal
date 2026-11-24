// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What an Azure Monitor metrics connector's settings say, read and checked in one place so that
//! saving, checking and pulling accept the same thing.

use kernel::{Error, Result};
use serde_json::Value;

use crate::infra::connectors::{
    ConnectorKind,
    cloud_metrics::{self, Schedule},
    config::text,
};

const KIND: ConnectorKind = ConnectorKind::AzureMonitorMetrics;
const DEFAULT_PERIOD_SECS: i64 = 300;
const DEFAULT_DELAY_SECS: i64 = 600;

/// The aggregations Azure Monitor keeps for a metric, as `getBatch` names them.
pub(super) const AGGREGATIONS: [&str; 5] = ["average", "minimum", "maximum", "total", "count"];

/// How many metric names one connector may pull.
const MAX_METRICS: usize = 100;

/// How many series of one metric of one resource a `filter` may split into by default. Azure
/// returns 10 when it is not told.
const DEFAULT_TOP: u32 = 100;
const MAX_TOP: u32 = 10_000;

/// The settings of one connector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Settings {
    pub tenant_id: String,
    pub client_id: String,
    pub client_secret: String,
    pub subscription_id: String,
    /// Lower case without spaces, as Azure names regions (`eastus`).
    pub region: String,
    /// The resource type, which is also the metric namespace (`Microsoft.Compute/virtualMachines`).
    pub resource_type: String,
    pub metrics: Vec<String>,
    pub aggregations: Vec<String>,
    pub filter: Option<String>,
    pub top: u32,
    pub schedule: Schedule,
}

impl Settings {
    pub(super) fn parse(config: &Value) -> Result<Self> {
        let tenant_id = cloud_metrics::required(config, "tenantId", KIND)?;
        let client_id = cloud_metrics::required(config, "clientId", KIND)?;
        let client_secret = cloud_metrics::required(config, "clientSecret", KIND)?;
        let subscription_id = cloud_metrics::required(config, "subscriptionId", KIND)?;
        let region = region(cloud_metrics::required(config, "region", KIND)?)?;
        let resource_type = cloud_metrics::required(config, "resourceType", KIND)?;

        // Both are put in a URL path: a stray `/` or `?` would send the request elsewhere.
        if !tenant_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.'))
        {
            return Err(Error::invalid(
                "`tenant_id` must be a directory ID or a domain such as contoso.onmicrosoft.com",
            ));
        }

        if !subscription_id
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() || ch == '-')
        {
            return Err(Error::invalid(
                "`subscription_id` must be a subscription ID (a GUID)",
            ));
        }

        if !is_resource_type(resource_type) {
            return Err(Error::invalid(format!(
                "`resource_type` must be a resource type such as Microsoft.Compute/virtualMachines, \
                 not `{resource_type}`"
            )));
        }

        Ok(Self {
            tenant_id: tenant_id.to_string(),
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            subscription_id: subscription_id.to_string(),
            region,
            resource_type: resource_type.to_string(),
            metrics: metrics(config)?,
            aggregations: aggregations(config)?,
            filter: Some(text(config, "filter"))
                .filter(|filter| !filter.is_empty())
                .map(str::to_string),
            top: top(config)?,
            schedule: schedule(config)?,
        })
    }

    /// The metric namespace without the `Microsoft.` every Azure resource type starts with, as
    /// the metric names carry it (`Compute/virtualMachines`).
    pub(super) fn namespace(&self) -> &str {
        let resource_type = self.resource_type.as_str();

        match resource_type.get(..10) {
            Some(prefix) if prefix.eq_ignore_ascii_case("microsoft.") => &resource_type[10..],
            _ => resource_type,
        }
    }
}

/// `East US` or `eastus` as Azure's region name, `eastus`.
fn region(value: &str) -> Result<String> {
    let region: String = value
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .map(|ch| ch.to_ascii_lowercase())
        .collect();

    // The region is the first label of the metrics endpoint's host name.
    if region.is_empty() || !region.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return Err(Error::invalid(format!(
            "`region` must be an Azure region such as eastus, not `{value}`"
        )));
    }

    Ok(region)
}

/// `Provider.Namespace/type[/subtype]`.
fn is_resource_type(value: &str) -> bool {
    let mut parts = value.split('/');
    let provider = parts.next().unwrap_or_default();
    let types: Vec<&str> = parts.collect();

    provider.contains('.')
        && !types.is_empty()
        && std::iter::once(provider).chain(types).all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
        })
}

fn metrics(config: &Value) -> Result<Vec<String>> {
    let metrics = cloud_metrics::list(config, "metrics");

    if metrics.is_empty() {
        return Err(Error::invalid(
            "`metrics` must name at least one metric of the resource type, such as Percentage CPU",
        ));
    }

    if metrics.len() > MAX_METRICS {
        return Err(Error::invalid(format!(
            "`metrics` names {} metrics: at most {MAX_METRICS} are pulled by one connector",
            metrics.len()
        )));
    }

    Ok(metrics)
}

fn aggregations(config: &Value) -> Result<Vec<String>> {
    let mut aggregations = Vec::new();

    for named in cloud_metrics::list(config, "aggregations") {
        let aggregation = named.to_ascii_lowercase();

        if !AGGREGATIONS.contains(&aggregation.as_str()) {
            return Err(Error::invalid(format!(
                "`aggregations` takes {}, not `{named}`",
                AGGREGATIONS.join(", ")
            )));
        }

        if !aggregations.contains(&aggregation) {
            aggregations.push(aggregation);
        }
    }

    if aggregations.is_empty() {
        aggregations.push("average".to_string());
    }

    Ok(aggregations)
}

fn top(config: &Value) -> Result<u32> {
    let invalid = || Error::invalid(format!("`top` must be a whole number from 1 to {MAX_TOP}"));

    let top = match config.get("top") {
        None | Some(Value::Null) => return Ok(DEFAULT_TOP),
        Some(Value::String(_)) if text(config, "top").is_empty() => return Ok(DEFAULT_TOP),
        Some(Value::String(_)) => text(config, "top").parse::<u32>().map_err(|_| invalid())?,
        Some(Value::Number(number)) => number
            .as_u64()
            .and_then(|top| u32::try_from(top).ok())
            .ok_or_else(invalid)?,
        Some(_) => return Err(invalid()),
    };

    if !(1..=MAX_TOP).contains(&top) {
        return Err(invalid());
    }

    Ok(top)
}

/// The schedule, its period one of the time grains Azure Monitor aggregates by.
fn schedule(config: &Value) -> Result<Schedule> {
    let schedule = Schedule::parse(config, DEFAULT_PERIOD_SECS, DEFAULT_DELAY_SECS)?;

    if time_grain(schedule.period_secs).is_none() {
        return Err(Error::invalid(
            "`period` must be one of Azure Monitor's time grains: 60, 300, 900, 1800, 3600, \
             21600, 43200 or 86400 seconds",
        ));
    }

    Ok(schedule)
}

/// A period as the ISO 8601 time grain `getBatch` takes as `interval`.
pub(super) fn time_grain(period_secs: i64) -> Option<&'static str> {
    Some(match period_secs {
        60 => "PT1M",
        300 => "PT5M",
        900 => "PT15M",
        1800 => "PT30M",
        3600 => "PT1H",
        21_600 => "PT6H",
        43_200 => "PT12H",
        86_400 => "P1D",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn config() -> Value {
        json!({
            "tenantId": "72f988bf-86f1-41af-91ab-2d7cd011db47",
            "clientId": "app",
            "clientSecret": "secret",
            "subscriptionId": "12345678-abcd-1234-abcd-123456789abc",
            "region": "East US",
            "resourceType": "Microsoft.Compute/virtualMachines",
            "metrics": "Percentage CPU, Network In Total",
        })
    }

    fn with(key: &str, value: Value) -> Value {
        let mut config = config();
        config[key] = value;
        config
    }

    #[test]
    fn the_defaults_fill_what_is_left_out() {
        let settings = Settings::parse(&config()).unwrap();

        assert_eq!(settings.region, "eastus");
        assert_eq!(settings.metrics, ["Percentage CPU", "Network In Total"]);
        assert_eq!(settings.aggregations, ["average"]);
        assert_eq!(settings.filter, None);
        assert_eq!(settings.top, DEFAULT_TOP);
        assert_eq!(settings.schedule.period_secs, 300);
        assert_eq!(settings.schedule.delay_secs, 600);
        assert_eq!(settings.namespace(), "Compute/virtualMachines");
    }

    #[test]
    fn aggregations_are_azure_s_five() {
        let settings = Settings::parse(&with(
            "aggregations",
            json!(["Maximum", "total", "maximum"]),
        ))
        .unwrap();

        assert_eq!(settings.aggregations, ["maximum", "total"]);
        assert!(Settings::parse(&with("aggregations", json!("p99"))).is_err());
    }

    #[test]
    fn the_period_is_one_of_azure_s_time_grains() {
        assert_eq!(time_grain(300), Some("PT5M"));
        assert_eq!(time_grain(86_400), Some("P1D"));
        assert_eq!(time_grain(120), None);
        assert!(Settings::parse(&with("period", json!(3600))).is_ok());
        assert!(Settings::parse(&with("period", json!(120))).is_err());
    }

    #[test]
    fn what_goes_into_a_url_is_checked() {
        assert!(Settings::parse(&with("tenantId", json!("contoso.onmicrosoft.com"))).is_ok());
        assert!(Settings::parse(&with("tenantId", json!("x/../y"))).is_err());
        assert!(Settings::parse(&with("subscriptionId", json!("sub?x=1"))).is_err());
        assert!(Settings::parse(&with("region", json!("east.us"))).is_err());
        assert!(Settings::parse(&with("resourceType", json!("virtualMachines"))).is_err());
        assert!(Settings::parse(&with("resourceType", json!("Microsoft.Compute/vm'"))).is_err());
    }

    #[test]
    fn metrics_and_credentials_are_required() {
        assert!(Settings::parse(&with("metrics", json!([]))).is_err());
        assert!(Settings::parse(&with("clientSecret", json!(""))).is_err());
        assert!(Settings::parse(&with("top", json!(0))).is_err());
        assert_eq!(Settings::parse(&with("top", json!("50"))).unwrap().top, 50);
    }

    #[test]
    fn a_namespace_without_microsoft_stays_as_it_is() {
        let settings =
            Settings::parse(&with("resourceType", json!("Contoso.Widgets/gadgets"))).unwrap();

        assert_eq!(settings.namespace(), "Contoso.Widgets/gadgets");
    }
}
