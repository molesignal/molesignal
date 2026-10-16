// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use crate::{
    domain::{
        alerting::incident::Severity, status_page::ComponentStatus, synthetics::MonitorState,
    },
    shared::{ids::Id, time::TimestampMicros},
};

/// How long the internal timeline of a business service reaches back.
pub const HEALTH_HISTORY_MICROS: i64 = 7 * 24 * 60 * 60 * 1_000_000;

/// The most periods the timeline keeps, however often the health changed.
pub const MAX_HEALTH_PERIODS: usize = 500;

/// What a business service depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    /// A service name, as traces and alerts carry it.
    TechnicalService,
    SyntheticMonitor,
    AlertRule,
    BusinessService,
}

impl DependencyKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TechnicalService => "technical_service",
            Self::SyntheticMonitor => "synthetic_monitor",
            Self::AlertRule => "alert_rule",
            Self::BusinessService => "business_service",
        }
    }
}

/// How much of a dependency's trouble reaches the business service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyImpact {
    /// Its status passes on as it is.
    Outage,
    /// It passes on at most degraded performance.
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessServiceDependency {
    pub kind: DependencyKind,
    /// The service name of a technical service, or the id of the monitor,
    /// alert rule or business service.
    pub target: String,
    pub impact: DependencyImpact,
}

/// The health of a business service, named like the status of the
/// status-page component that can stand for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessServiceStatus {
    Operational,
    DegradedPerformance,
    PartialOutage,
    MajorOutage,
}

impl BusinessServiceStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Operational => "operational",
            Self::DegradedPerformance => "degraded_performance",
            Self::PartialOutage => "partial_outage",
            Self::MajorOutage => "major_outage",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "operational" => Some(Self::Operational),
            "degraded_performance" => Some(Self::DegradedPerformance),
            "partial_outage" => Some(Self::PartialOutage),
            "major_outage" => Some(Self::MajorOutage),
            _ => None,
        }
    }

    pub const fn rank(self) -> u8 {
        match self {
            Self::Operational => 0,
            Self::DegradedPerformance => 1,
            Self::PartialOutage => 2,
            Self::MajorOutage => 3,
        }
    }

    pub fn worst(self, other: Self) -> Self {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }

    /// An alert of this severity: critical is a major outage, error a partial
    /// one, warning degraded performance; info changes nothing.
    pub const fn from_severity(severity: Severity) -> Self {
        match severity {
            Severity::Info => Self::Operational,
            Severity::Warning => Self::DegradedPerformance,
            Severity::Error => Self::PartialOutage,
            Severity::Critical => Self::MajorOutage,
        }
    }

    /// A monitor in this state. A flaky monitor passes intermittently and
    /// changes nothing; an unknown one has not run.
    pub const fn from_monitor_state(state: MonitorState) -> Self {
        match state {
            MonitorState::Failing => Self::MajorOutage,
            MonitorState::Degraded => Self::DegradedPerformance,
            MonitorState::Healthy | MonitorState::Flaky | MonitorState::Unknown => {
                Self::Operational
            }
        }
    }

    /// The severity of an automation observation of this status.
    pub const fn severity(self) -> Severity {
        match self {
            Self::Operational => Severity::Info,
            Self::DegradedPerformance => Severity::Warning,
            Self::PartialOutage => Severity::Error,
            Self::MajorOutage => Severity::Critical,
        }
    }

    pub const fn component_status(self) -> ComponentStatus {
        match self {
            Self::Operational => ComponentStatus::Operational,
            Self::DegradedPerformance => ComponentStatus::DegradedPerformance,
            Self::PartialOutage => ComponentStatus::PartialOutage,
            Self::MajorOutage => ComponentStatus::MajorOutage,
        }
    }

    pub fn passed_on(self, impact: DependencyImpact) -> Self {
        match impact {
            DependencyImpact::Outage => self,
            DependencyImpact::Degraded if self == Self::Operational => self,
            DependencyImpact::Degraded => Self::DegradedPerformance,
        }
    }
}

/// An alert incident behind a cause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CauseIncident {
    pub id: Id,
    pub summary: String,
    pub severity: Severity,
}

/// A dependency that is not operational, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthCause {
    pub kind: DependencyKind,
    pub target: String,
    pub status: BusinessServiceStatus,
    /// The open alert incidents behind it, most severe first, at most five.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incidents: Vec<CauseIncident>,
    /// How many open alert incidents are behind it in all.
    #[serde(default)]
    pub incident_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monitor_state: Option<MonitorState>,
}

/// A period the business service spent in one status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthPeriod {
    pub status: BusinessServiceStatus,
    pub started_at: TimestampMicros,
    pub ended_at: Option<TimestampMicros>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessServiceHealth {
    pub status: BusinessServiceStatus,
    /// The dependencies that are not operational, worst first.
    pub causes: Vec<HealthCause>,
    /// When the status last changed; `None` before the first evaluation.
    pub changed_at: Option<TimestampMicros>,
    pub evaluated_at: Option<TimestampMicros>,
    /// The periods of the last seven days, oldest first; the last is open.
    pub history: Vec<HealthPeriod>,
}

impl BusinessServiceHealth {
    pub fn unevaluated() -> Self {
        Self {
            status: BusinessServiceStatus::Operational,
            causes: Vec::new(),
            changed_at: None,
            evaluated_at: None,
            history: Vec::new(),
        }
    }

    /// The health after an evaluation at `at` found `status` and `causes`.
    pub fn evaluated(
        &self,
        status: BusinessServiceStatus,
        causes: Vec<HealthCause>,
        at: TimestampMicros,
    ) -> Self {
        let first = self.evaluated_at.is_none();

        if !first && status == self.status {
            return Self {
                causes,
                evaluated_at: Some(at),
                ..self.clone()
            };
        }

        let horizon = at.0.saturating_sub(HEALTH_HISTORY_MICROS);

        let mut history: Vec<HealthPeriod> = self
            .history
            .iter()
            .cloned()
            .map(|mut period| {
                if period.ended_at.is_none() {
                    period.ended_at = Some(at);
                }

                period
            })
            .filter(|period| period.ended_at.is_none_or(|ended| ended.0 >= horizon))
            .collect();

        history.push(HealthPeriod {
            status,
            started_at: at,
            ended_at: None,
        });

        if history.len() > MAX_HEALTH_PERIODS {
            history.drain(..history.len() - MAX_HEALTH_PERIODS);
        }

        Self {
            status,
            causes,
            changed_at: Some(at),
            evaluated_at: Some(at),
            history,
        }
    }

    /// Whether this health records another status than `before` did, or is the
    /// first one recorded: the change that effects and the compare-and-set act on.
    pub fn status_changed_from(&self, before: &Self) -> bool {
        before.evaluated_at.is_none() || before.status != self.status
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessService {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub description: String,
    pub dependencies: Vec<BusinessServiceDependency>,
    pub health: BusinessServiceHealth,
    pub created_by: Option<Id>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 60 * 60 * 1_000_000;

    #[test]
    fn a_degraded_dependency_passes_on_at_most_degraded_performance() {
        let outage = BusinessServiceStatus::MajorOutage;
        assert_eq!(outage.passed_on(DependencyImpact::Outage), outage);
        assert_eq!(
            outage.passed_on(DependencyImpact::Degraded),
            BusinessServiceStatus::DegradedPerformance
        );
        assert_eq!(
            BusinessServiceStatus::Operational.passed_on(DependencyImpact::Degraded),
            BusinessServiceStatus::Operational
        );
    }

    #[test]
    fn the_first_evaluation_opens_a_period_and_a_change_closes_it() {
        let first = BusinessServiceHealth::unevaluated().evaluated(
            BusinessServiceStatus::Operational,
            Vec::new(),
            TimestampMicros(10 * HOUR),
        );
        assert!(first.status_changed_from(&BusinessServiceHealth::unevaluated()));
        assert_eq!(first.history.len(), 1);

        let same = first.evaluated(
            BusinessServiceStatus::Operational,
            Vec::new(),
            TimestampMicros(11 * HOUR),
        );
        assert!(!same.status_changed_from(&first));
        assert_eq!(same.changed_at, Some(TimestampMicros(10 * HOUR)));
        assert_eq!(same.evaluated_at, Some(TimestampMicros(11 * HOUR)));

        let outage = same.evaluated(
            BusinessServiceStatus::MajorOutage,
            Vec::new(),
            TimestampMicros(12 * HOUR),
        );
        assert!(outage.status_changed_from(&same));
        assert_eq!(outage.history.len(), 2);
        assert_eq!(outage.history[0].ended_at, Some(TimestampMicros(12 * HOUR)));
        assert_eq!(outage.history[1].ended_at, None);
    }

    #[test]
    fn the_timeline_forgets_periods_that_ended_more_than_a_week_ago() {
        let mut health = BusinessServiceHealth::unevaluated().evaluated(
            BusinessServiceStatus::Operational,
            Vec::new(),
            TimestampMicros(0),
        );

        health = health.evaluated(
            BusinessServiceStatus::PartialOutage,
            Vec::new(),
            TimestampMicros(HOUR),
        );
        health = health.evaluated(
            BusinessServiceStatus::Operational,
            Vec::new(),
            TimestampMicros(2 * HOUR),
        );

        let later = TimestampMicros(2 * HOUR + HEALTH_HISTORY_MICROS + HOUR);
        health = health.evaluated(
            BusinessServiceStatus::DegradedPerformance,
            Vec::new(),
            later,
        );

        let statuses: Vec<_> = health.history.iter().map(|period| period.status).collect();
        assert_eq!(
            statuses,
            [
                BusinessServiceStatus::Operational,
                BusinessServiceStatus::DegradedPerformance
            ]
        );
    }
}
