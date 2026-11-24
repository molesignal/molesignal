// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Azure Monitor alerts: the webhook action of an action group, in the common alert schema
//! (<https://learn.microsoft.com/en-us/azure/azure-monitor/alerts/alerts-common-schema>).
//!
//! - Only `schemaId: azureMonitorCommonAlertSchema` is read. The older per-signal payloads are
//!   refused with a reason that says to turn the common alert schema on for the webhook.
//! - `data.essentials.monitorCondition`: `Fired` triggers, `Resolved` resolves. An alert that does
//!   not keep state (activity log alerts) only fires, so a person resolves it.
//! - One incident per alert: the dedup key is `alertId`, which holds the Azure subscription.
//! - `severity`: `Sev0` is critical, `Sev1` error, `Sev2` warning, `Sev3` and `Sev4` info.
//! - The rule, targets, condition and custom properties go in labels and annotations.

use std::collections::BTreeMap;

use domain::alerting::{
    incident::Severity,
    source::{ExternalAction, ExternalAlert},
};
use kernel::{Error, Result};
use serde_json::Value;

use super::{Parsed, field, json, rfc3339, strings, text, web_address};

/// The schema the webhook must send.
const COMMON_SCHEMA: &str = "azureMonitorCommonAlertSchema";

pub(super) fn parse(body: &[u8]) -> Result<Parsed> {
    let payload = json(body)?;

    if field(&payload, "schemaId").as_deref() != Some(COMMON_SCHEMA) {
        return Err(Error::invalid(
            "not the Azure Monitor common alert schema: turn on Enable the common alert schema \
             for the webhook action in the action group",
        ));
    }

    let essentials = payload
        .pointer("/data/essentials")
        .cloned()
        .unwrap_or(Value::Null);

    let Some(alert_id) = field(&essentials, "alertId") else {
        return Err(Error::invalid(
            "the Azure Monitor alert has no `data.essentials.alertId`",
        ));
    };

    let action = match field(&essentials, "monitorCondition").as_deref() {
        Some("Fired") => ExternalAction::Trigger,
        Some("Resolved") => ExternalAction::Resolve,
        other => {
            return Ok(Parsed::nothing(format!(
                "Azure Monitor condition {} is neither Fired nor Resolved",
                other.unwrap_or("(none)")
            )));
        }
    };

    let time_key = match action {
        ExternalAction::Resolve => "resolvedDateTime",
        _ => "firedDateTime",
    };

    let occurred_at = field(&essentials, time_key)
        .or_else(|| field(&essentials, "firedDateTime"))
        .as_deref()
        .and_then(rfc3339);

    let context = payload
        .pointer("/data/alertContext")
        .cloned()
        .unwrap_or(Value::Null);
    let criterion = context
        .pointer("/condition/allOf/0")
        .cloned()
        .unwrap_or(Value::Null);

    Ok(Parsed::alerts(vec![ExternalAlert {
        action,
        summary: summary(&essentials),
        severity: field(&essentials, "severity")
            .as_deref()
            .and_then(severity_of),
        labels: labels(&payload, &essentials, &criterion, &alert_id),
        annotations: annotations(&essentials, &criterion),
        dedup_key: alert_id,
        occurred_at,
        no_data: false,
        members: Vec::new(),
        priority: None,
        urgency: None,
    }]))
}

fn severity_of(level: &str) -> Option<Severity> {
    match level.trim().to_ascii_lowercase().as_str() {
        "sev0" => Some(Severity::Critical),
        "sev1" => Some(Severity::Error),
        "sev2" => Some(Severity::Warning),
        "sev3" | "sev4" => Some(Severity::Info),
        _ => None,
    }
}

/// `rule · resource`: the rule's name and the first resource it fired on.
fn summary(essentials: &Value) -> String {
    let rule = field(essentials, "alertRule").unwrap_or_else(|| "Azure Monitor alert".into());
    let item = first(essentials.get("configurationItems"));

    match item {
        Some(item) => format!("{rule} · {item}"),
        None => rule,
    }
}

fn first(value: Option<&Value>) -> Option<String> {
    value?.as_array()?.iter().find_map(text)
}

/// The value after `/<segment>/` in an Azure resource ID, ignoring case.
fn segment(id: &str, name: &str) -> Option<String> {
    let mut parts = id.split('/');

    parts.find(|part| part.eq_ignore_ascii_case(name))?;
    parts
        .next()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn labels(
    payload: &Value,
    essentials: &Value,
    criterion: &Value,
    alert_id: &str,
) -> BTreeMap<String, String> {
    let mut labels = strings(payload.pointer("/data/customProperties"));

    for dimension in criterion
        .get("dimensions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let (Some(name), Some(value)) = (field(dimension, "name"), field(dimension, "value")) {
            labels.entry(name).or_insert(value);
        }
    }

    let target = first(essentials.get("alertTargetIDs"));

    let extra = [
        ("azure.subscription_id", segment(alert_id, "subscriptions")),
        (
            "azure.resource_group",
            target
                .as_deref()
                .and_then(|target| segment(target, "resourcegroups")),
        ),
        (
            "azure.resource",
            first(essentials.get("configurationItems")),
        ),
        ("azure.signal_type", field(essentials, "signalType")),
        (
            "azure.monitoring_service",
            field(essentials, "monitoringService"),
        ),
        ("azure.metric", field(criterion, "metricName")),
    ];

    labels.extend(
        extra
            .into_iter()
            .filter_map(|(key, value)| value.map(|value| (key.to_string(), value))),
    );

    labels
}

fn annotations(essentials: &Value, criterion: &Value) -> BTreeMap<String, String> {
    let condition = field(criterion, "metricName")
        .or_else(|| field(criterion, "searchQuery"))
        .map(
            |what| match (field(criterion, "operator"), field(criterion, "threshold")) {
                (Some(operator), Some(threshold)) => format!("{what} {operator} {threshold}"),
                _ => what,
            },
        );

    let targets = essentials
        .get("alertTargetIDs")
        .and_then(Value::as_array)
        .map(|targets| {
            targets
                .iter()
                .filter_map(text)
                .collect::<Vec<_>>()
                .join("\n")
        });

    [
        ("message", field(essentials, "description")),
        ("alert_rule", field(essentials, "alertRule")),
        ("condition", condition),
        ("value", field(criterion, "metricValue")),
        ("targets", targets),
        (
            "azure_url",
            field(essentials, "investigationLink").and_then(|url| web_address(&url)),
        ),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.map(|value| (key.to_string(), value)))
    .collect()
}

#[cfg(test)]
mod tests {
    use kernel::time::TimestampMicros;
    use serde_json::json;

    use super::*;

    /// The metric alert of the common alert schema docs.
    fn metric_alert(condition: &str) -> Value {
        json!({
            "schemaId": "azureMonitorCommonAlertSchema",
            "data": {
                "essentials": {
                    "alertId": "/subscriptions/11111111-1111-1111-1111-111111111111/providers/Microsoft.AlertsManagement/alerts/b9569717-bc32-442f-add5-83a997729330",
                    "alertRule": "WCUS-R2-Gen2",
                    "severity": "Sev3",
                    "signalType": "Metric",
                    "monitorCondition": condition,
                    "monitoringService": "Platform",
                    "alertTargetIDs": [
                        "/subscriptions/11111111-1111-1111-1111-111111111111/resourcegroups/pipelinealertrg/providers/microsoft.compute/virtualmachines/wcus-r2-gen2"
                    ],
                    "configurationItems": ["wcus-r2-gen2"],
                    "originAlertId": "3f2d4487-b0fc-4125-8bd5-7ad17384221e_PipeLineAlertRG_microsoft.insights_metricAlerts_WCUS-R2-Gen2_-117781227",
                    "firedDateTime": "2019-03-22T13:58:24.3713213Z",
                    "resolvedDateTime": "2019-03-22T14:03:16.2246313Z",
                    "description": "CPU above 25% on the gen2 VM",
                    "essentialsVersion": "1.0",
                    "alertContextVersion": "1.0"
                },
                "alertContext": {
                    "properties": null,
                    "conditionType": "SingleResourceMultipleMetricCriteria",
                    "condition": {
                        "windowSize": "PT5M",
                        "allOf": [{
                            "metricName": "Percentage CPU",
                            "metricNamespace": "Microsoft.Compute/virtualMachines",
                            "operator": "GreaterThan",
                            "threshold": "25",
                            "timeAggregation": "Average",
                            "dimensions": [{"name": "ResourceId", "value": "3efad9dc-3d50-4eac-9c87-8b3fd6f97e4e"}],
                            "metricValue": 7.727
                        }]
                    }
                },
                "customProperties": {"Key1": "Value1"}
            }
        })
    }

    fn one(payload: &Value) -> ExternalAlert {
        parse(payload.to_string().as_bytes())
            .unwrap()
            .alerts
            .into_iter()
            .next()
            .unwrap()
    }

    #[test]
    fn a_fired_metric_alert_opens_with_its_rule_and_target() {
        let alert = one(&metric_alert("Fired"));

        assert_eq!(alert.action, ExternalAction::Trigger);
        assert_eq!(alert.summary, "WCUS-R2-Gen2 · wcus-r2-gen2");
        assert_eq!(alert.severity, Some(Severity::Info));
        assert!(
            alert
                .dedup_key
                .ends_with("/alerts/b9569717-bc32-442f-add5-83a997729330")
        );
        assert_eq!(
            alert.labels["azure.subscription_id"],
            "11111111-1111-1111-1111-111111111111"
        );
        assert_eq!(alert.labels["azure.resource_group"], "pipelinealertrg");
        assert_eq!(alert.labels["azure.resource"], "wcus-r2-gen2");
        assert_eq!(alert.labels["azure.signal_type"], "Metric");
        assert_eq!(alert.labels["azure.metric"], "Percentage CPU");
        assert_eq!(
            alert.labels["ResourceId"],
            "3efad9dc-3d50-4eac-9c87-8b3fd6f97e4e"
        );
        assert_eq!(alert.labels["Key1"], "Value1");
        assert_eq!(
            alert.annotations["condition"],
            "Percentage CPU GreaterThan 25"
        );
        assert_eq!(alert.annotations["value"], "7.727");
        assert_eq!(alert.annotations["message"], "CPU above 25% on the gen2 VM");
        assert_eq!(
            alert.occurred_at,
            Some(TimestampMicros(1_553_263_104_371_321))
        );
    }

    #[test]
    fn resolved_closes_the_same_alert_at_its_resolve_time() {
        let fired = one(&metric_alert("Fired"));
        let resolved = one(&metric_alert("Resolved"));

        assert_eq!(resolved.action, ExternalAction::Resolve);
        assert_eq!(resolved.dedup_key, fired.dedup_key);
        assert_eq!(
            resolved.occurred_at,
            Some(TimestampMicros(1_553_263_396_224_631))
        );
    }

    #[test]
    fn severities_count_from_sev0() {
        assert_eq!(severity_of("Sev0"), Some(Severity::Critical));
        assert_eq!(severity_of("Sev1"), Some(Severity::Error));
        assert_eq!(severity_of("Sev2"), Some(Severity::Warning));
        assert_eq!(severity_of("Sev4"), Some(Severity::Info));
        assert_eq!(severity_of("Sev9"), None);
    }

    #[test]
    fn an_older_schema_is_refused_and_says_how_to_switch() {
        // A classic metric alert, sent without the common schema.
        let classic = json!({
            "status": "Activated",
            "context": {"id": "/subscriptions/1/resourceGroups/rg/providers/microsoft.insights/alertrules/cpu", "name": "cpu"}
        });

        let error = parse(classic.to_string().as_bytes())
            .unwrap_err()
            .to_string();

        assert!(error.contains("common alert schema"), "{error}");

        let activity_log = json!({"schemaId": "Microsoft.Insights/activityLogs", "data": {}});

        assert!(parse(activity_log.to_string().as_bytes()).is_err());
    }
}
