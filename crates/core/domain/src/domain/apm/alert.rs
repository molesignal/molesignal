// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What an alert rule reads from APM: a service, or one of its endpoints,
//! and the figure it watches, in the unit people set thresholds in.

use serde::{Deserialize, Serialize};

use super::TransactionIdentity;
use crate::shared::{Error, Result, ids::Id};

/// The longest service, namespace or environment name an APM rule names.
const MAX_NAME_BYTES: usize = 255;

/// A figure of a service or an endpoint over the rule's window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApmAlertMetric {
    /// Failed requests over all requests, in percent (0–100).
    ErrorRate,
    /// Latency percentiles and the mean, in milliseconds.
    P50,
    P90,
    P95,
    P99,
    Average,
    /// Requests a second.
    Throughput,
    RequestCount,
    ErrorCount,
    /// Error groups first seen in the window, a new kind of failure.
    NewErrorGroups,
    /// Apdex against the default threshold, 0–1.
    Apdex,
    /// How many times faster than the objective allows an SLO spends its
    /// error budget over the rule's window (1 = exactly on budget).
    SloBurnRate,
}

impl ApmAlertMetric {
    /// The unit a value of the figure is in, as rule pages say it.
    pub fn unit(self) -> &'static str {
        match self {
            Self::ErrorRate => "percent",
            Self::P50 | Self::P90 | Self::P95 | Self::P99 | Self::Average => "milliseconds",
            Self::Throughput => "requests_per_second",
            Self::RequestCount | Self::ErrorCount | Self::NewErrorGroups => "count",
            Self::Apdex => "score",
            Self::SloBurnRate => "burn_rate",
        }
    }
}

/// The APM figure an alert rule watches, in place of a query statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApmAlertQuery {
    #[serde(default)]
    pub namespace: Option<String>,
    pub service: String,
    /// Every environment of the service when absent.
    #[serde(default)]
    pub environment: Option<String>,
    /// One endpoint of the service; the whole service when absent.
    #[serde(default)]
    pub transaction: Option<TransactionIdentity>,
    pub metric: ApmAlertMetric,
    /// The SLO whose burn rate is read; required for `slo_burn_rate` only.
    #[serde(default)]
    pub slo_id: Option<Id>,
    /// A second, shorter window the burn rate must pass the threshold over
    /// as well, so a spike that is already over stops paging: the rule's
    /// period is the long window.
    #[serde(default)]
    pub short_window_secs: Option<u32>,
}

impl ApmAlertQuery {
    pub fn validate(&self, period_secs: u32) -> Result<()> {
        let named = |value: &str, what: &str| -> Result<()> {
            if value.trim().is_empty() || value.len() > MAX_NAME_BYTES || value.contains('\0') {
                return Err(Error::invalid(format!("APM alert {what} is invalid")));
            }

            Ok(())
        };

        named(&self.service, "service")?;

        if let Some(namespace) = &self.namespace {
            named(namespace, "namespace")?;
        }

        if let Some(environment) = &self.environment {
            named(environment, "environment")?;
        }

        if let Some(transaction) = &self.transaction {
            named(&transaction.name, "endpoint")?;
        }

        match (self.metric, &self.slo_id) {
            (ApmAlertMetric::SloBurnRate, None) => {
                return Err(Error::invalid("an SLO burn-rate alert names its SLO"));
            }
            (ApmAlertMetric::SloBurnRate, Some(_)) => {}
            (_, Some(_)) => return Err(Error::invalid("only an SLO burn-rate alert names an SLO")),
            (_, None) => {}
        }

        if let Some(short) = self.short_window_secs
            && (self.metric != ApmAlertMetric::SloBurnRate || short == 0 || short >= period_secs)
        {
            return Err(Error::invalid(
                "a short window is for burn rates and must be shorter than the rule's period",
            ));
        }

        Ok(())
    }

    /// What the rule watches, for the statement field of rule lists.
    pub fn describe(&self) -> String {
        let target = match &self.transaction {
            Some(transaction) => format!("{} {}", self.service, transaction.name),
            None => self.service.clone(),
        };
        let metric = serde_json::to_value(self.metric)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();

        format!("apm:{metric} {target}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::apm::TransactionKind;

    fn query(metric: ApmAlertMetric) -> ApmAlertQuery {
        ApmAlertQuery {
            namespace: Some("shop".into()),
            service: "checkout".into(),
            environment: Some("prod".into()),
            transaction: None,
            metric,
            slo_id: None,
            short_window_secs: None,
        }
    }

    #[test]
    fn names_a_service_and_an_slo_only_for_burn_rates() {
        assert!(query(ApmAlertMetric::ErrorRate).validate(300).is_ok());
        assert!(
            ApmAlertQuery {
                service: " ".into(),
                ..query(ApmAlertMetric::P95)
            }
            .validate(300)
            .is_err()
        );
        assert!(query(ApmAlertMetric::SloBurnRate).validate(3_600).is_err());

        let burn = ApmAlertQuery {
            slo_id: Some(Id::from_string("slo-1")),
            short_window_secs: Some(300),
            ..query(ApmAlertMetric::SloBurnRate)
        };
        assert!(burn.validate(3_600).is_ok());
        assert!(burn.validate(300).is_err());
        assert!(
            ApmAlertQuery {
                slo_id: Some(Id::from_string("slo-1")),
                ..query(ApmAlertMetric::P95)
            }
            .validate(300)
            .is_err()
        );
    }

    #[test]
    fn describes_what_it_watches() {
        let endpoint = ApmAlertQuery {
            transaction: Some(TransactionIdentity {
                name: "GET /cart".into(),
                kind: TransactionKind::Http,
            }),
            ..query(ApmAlertMetric::P95)
        };
        assert_eq!(endpoint.describe(), "apm:p95 checkout GET /cart");
        assert_eq!(
            query(ApmAlertMetric::ErrorRate).describe(),
            "apm:error_rate checkout"
        );
    }
}
