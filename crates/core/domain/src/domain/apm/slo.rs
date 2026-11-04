// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Service level objectives over APM's aggregates: the share of good requests
//! a service, or one of its endpoints, promises over a rolling window.

use serde::{Deserialize, Serialize};

use super::TransactionIdentity;
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

/// The windows an objective can be held over, in days.
pub const SLO_WINDOW_DAYS: [u16; 3] = [7, 28, 30];

/// What makes a request good.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SloKind {
    /// It did not fail.
    Availability,
    /// It did not fail and took at most the threshold.
    Latency,
}

impl SloKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Availability => "availability",
            Self::Latency => "latency",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "availability" => Some(Self::Availability),
            "latency" => Some(Self::Latency),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLevelObjective {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub namespace: String,
    pub service: String,
    pub environment: String,
    /// One endpoint of the service; every request of the service when absent.
    #[serde(default)]
    pub transaction: Option<TransactionIdentity>,
    pub kind: SloKind,
    /// The longest a good request of a latency objective takes.
    #[serde(default)]
    pub latency_threshold: Option<u64>,
    /// The share of good requests promised, such as 0.999.
    pub objective: f64,
    pub window_days: u16,
    pub created_by: Id,
    pub created_at: TimestampMicros,
    pub updated_by: Id,
    pub updated_at: TimestampMicros,
}

impl ServiceLevelObjective {
    pub fn validate(&self) -> Result<()> {
        let named = |value: &str, limit: usize, what: &str| -> Result<()> {
            if value.trim().is_empty() || value.len() > limit || value.contains('\0') {
                return Err(Error::invalid(format!("SLO {what} is invalid")));
            }

            Ok(())
        };

        named(&self.name, 255, "name")?;
        named(&self.namespace, 128, "namespace")?;
        named(&self.service, 255, "service")?;
        named(&self.environment, 128, "environment")?;

        if let Some(transaction) = &self.transaction {
            named(&transaction.name, 255, "endpoint")?;
        }

        if self.description.len() > 2_000 {
            return Err(Error::invalid("SLO description is longer than 2000 bytes"));
        }

        if !self.objective.is_finite() || self.objective <= 0.0 || self.objective >= 1.0 {
            return Err(Error::invalid(
                "SLO objective must be between 0 and 1, exclusive",
            ));
        }

        if !SLO_WINDOW_DAYS.contains(&self.window_days) {
            return Err(Error::invalid("SLO window must be 7, 28 or 30 days"));
        }

        match (self.kind, self.latency_threshold) {
            (SloKind::Latency, Some(threshold)) if threshold > 0 => Ok(()),
            (SloKind::Latency, _) => {
                Err(Error::invalid("a latency SLO needs a positive threshold"))
            }
            (SloKind::Availability, None) => Ok(()),
            (SloKind::Availability, Some(_)) => Err(Error::invalid(
                "an availability SLO has no latency threshold",
            )),
        }
    }

    /// The share of requests allowed to be bad.
    pub fn budget(&self) -> f64 {
        1.0 - self.objective
    }

    pub fn window_micros(&self) -> i64 {
        i64::from(self.window_days) * 86_400_000_000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slo(kind: SloKind, threshold: Option<u64>) -> ServiceLevelObjective {
        ServiceLevelObjective {
            id: Id::from_string("slo"),
            name: "Checkout availability".into(),
            description: String::new(),
            namespace: "shop".into(),
            service: "checkout".into(),
            environment: "prod".into(),
            transaction: None,
            kind,
            latency_threshold: threshold,
            objective: 0.999,
            window_days: 30,
            created_by: Id::from_string("u"),
            created_at: TimestampMicros(0),
            updated_by: Id::from_string("u"),
            updated_at: TimestampMicros(0),
        }
    }

    #[test]
    fn holds_an_objective_between_zero_and_one_over_a_known_window() {
        assert!(slo(SloKind::Availability, None).validate().is_ok());
        assert!(slo(SloKind::Latency, Some(300_000)).validate().is_ok());
        assert!(slo(SloKind::Latency, None).validate().is_err());
        assert!(slo(SloKind::Availability, Some(1)).validate().is_err());
        assert!(
            ServiceLevelObjective {
                objective: 1.0,
                ..slo(SloKind::Availability, None)
            }
            .validate()
            .is_err()
        );
        assert!(
            ServiceLevelObjective {
                window_days: 14,
                ..slo(SloKind::Availability, None)
            }
            .validate()
            .is_err()
        );
        assert!((slo(SloKind::Availability, None).budget() - 0.001).abs() < 1e-12);
    }
}
