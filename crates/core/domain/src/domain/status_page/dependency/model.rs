// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{
    domain::{
        alerting::incident::Severity, status_page::ComponentStatus, synthetics::MonitorState,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// The most dependencies one component can have.
pub const MAX_COMPONENT_DEPENDENCIES: usize = 100;

/// The longest name of a dependency target.
const MAX_TARGET_CHARS: usize = 255;

/// What a component depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    /// A service name, as traces and alerts carry it.
    TechnicalService,
    SyntheticMonitor,
    AlertRule,
}

impl DependencyKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TechnicalService => "technical_service",
            Self::SyntheticMonitor => "synthetic_monitor",
            Self::AlertRule => "alert_rule",
        }
    }
}

/// How much of a dependency's trouble reaches the component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyImpact {
    /// Its status passes on as it is.
    Outage,
    /// It passes on at most degraded performance.
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentDependency {
    pub kind: DependencyKind,
    /// The service name of a technical service, or the id of the monitor or
    /// alert rule.
    pub target: String,
    pub impact: DependencyImpact,
}

/// How the health of its dependencies reaches a component.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyMode {
    /// The component's status follows the health.
    #[default]
    Follow,
    /// The health is a source of the page's automation rules, which publish
    /// incidents on the component.
    Automation,
}

impl DependencyMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Follow => "follow",
            Self::Automation => "automation",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "follow" => Some(Self::Follow),
            "automation" => Some(Self::Automation),
            _ => None,
        }
    }
}

/// The health worked out from a component's dependencies, named like the
/// statuses a component can take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyStatus {
    Operational,
    DegradedPerformance,
    PartialOutage,
    MajorOutage,
}

impl DependencyStatus {
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
#[serde(rename_all = "camelCase")]
pub struct HealthCause {
    pub kind: DependencyKind,
    pub target: String,
    pub status: DependencyStatus,
    /// The open alert incidents behind it, most severe first, at most five.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incidents: Vec<CauseIncident>,
    /// How many open alert incidents are behind it in all.
    #[serde(default)]
    pub incident_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monitor_state: Option<MonitorState>,
}

/// The health last worked out from a component's dependencies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyHealth {
    pub status: DependencyStatus,
    /// The dependencies that are not operational, worst first.
    pub causes: Vec<HealthCause>,
    /// When the status last changed.
    pub changed_at: TimestampMicros,
    pub evaluated_at: TimestampMicros,
}

impl DependencyHealth {
    /// The health after an evaluation at `at` found `status` and `causes`,
    /// given the one recorded before, if any.
    pub fn after(
        before: Option<&Self>,
        status: DependencyStatus,
        causes: Vec<HealthCause>,
        at: TimestampMicros,
    ) -> Self {
        let changed_at = match before {
            Some(before) if before.status == status => before.changed_at,
            _ => at,
        };

        Self {
            status,
            causes,
            changed_at,
            evaluated_at: at,
        }
    }

    /// Whether this health records another status than `before` did, or is the
    /// first one recorded: the change that effects and the compare-and-set act on.
    pub fn changed_from(&self, before: Option<&Self>) -> bool {
        before.is_none_or(|before| before.status != self.status)
    }
}

/// What recording an evaluation did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthWrite {
    /// The status is the one stored; causes and the evaluation time were updated.
    Unchanged,
    /// The status changed through this write.
    Changed,
    /// Another evaluation changed the status first, or the dependencies were
    /// changed meanwhile; nothing was written.
    Lost,
}

/// The dependencies as they are kept: targets trimmed, each named once, a
/// technical service whatever the case of its name.
pub fn normalize_dependencies(
    dependencies: Vec<ComponentDependency>,
) -> Result<Vec<ComponentDependency>> {
    if dependencies.len() > MAX_COMPONENT_DEPENDENCIES {
        return Err(Error::invalid(
            "a component can have at most 100 dependencies",
        ));
    }

    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(dependencies.len());

    for mut dependency in dependencies {
        dependency.target = dependency.target.trim().to_string();

        if dependency.target.is_empty()
            || dependency.target.chars().count() > MAX_TARGET_CHARS
            || dependency.target.chars().any(char::is_control)
        {
            return Err(Error::invalid(
                "a dependency must name its target in 1 to 255 characters",
            ));
        }

        let key = match dependency.kind {
            DependencyKind::TechnicalService => dependency.target.to_lowercase(),
            _ => dependency.target.clone(),
        };

        if !seen.insert((dependency.kind, key)) {
            return Err(Error::invalid(format!(
                "{} is named twice",
                dependency.target
            )));
        }

        normalized.push(dependency);
    }

    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 60 * 60 * 1_000_000;

    fn on(kind: DependencyKind, target: &str) -> ComponentDependency {
        ComponentDependency {
            kind,
            target: target.into(),
            impact: DependencyImpact::Outage,
        }
    }

    #[test]
    fn a_degraded_dependency_passes_on_at_most_degraded_performance() {
        let outage = DependencyStatus::MajorOutage;
        assert_eq!(outage.passed_on(DependencyImpact::Outage), outage);
        assert_eq!(
            outage.passed_on(DependencyImpact::Degraded),
            DependencyStatus::DegradedPerformance
        );
        assert_eq!(
            DependencyStatus::Operational.passed_on(DependencyImpact::Degraded),
            DependencyStatus::Operational
        );
    }

    #[test]
    fn the_first_evaluation_is_a_change_and_the_same_status_keeps_its_time() {
        let first = DependencyHealth::after(
            None,
            DependencyStatus::Operational,
            Vec::new(),
            TimestampMicros(10 * HOUR),
        );
        assert!(first.changed_from(None));

        let same = DependencyHealth::after(
            Some(&first),
            DependencyStatus::Operational,
            Vec::new(),
            TimestampMicros(11 * HOUR),
        );
        assert!(!same.changed_from(Some(&first)));
        assert_eq!(same.changed_at, TimestampMicros(10 * HOUR));
        assert_eq!(same.evaluated_at, TimestampMicros(11 * HOUR));

        let outage = DependencyHealth::after(
            Some(&same),
            DependencyStatus::MajorOutage,
            Vec::new(),
            TimestampMicros(12 * HOUR),
        );
        assert!(outage.changed_from(Some(&same)));
        assert_eq!(outage.changed_at, TimestampMicros(12 * HOUR));
    }

    #[test]
    fn a_dependency_is_named_once_and_technical_names_ignore_case() {
        let kept = normalize_dependencies(vec![
            on(DependencyKind::TechnicalService, "  checkout-api "),
            on(DependencyKind::AlertRule, "rule-1"),
        ])
        .unwrap();
        assert_eq!(kept[0].target, "checkout-api");

        assert!(
            normalize_dependencies(vec![
                on(DependencyKind::TechnicalService, "Checkout-API"),
                on(DependencyKind::TechnicalService, "checkout-api"),
            ])
            .is_err()
        );
        assert!(normalize_dependencies(vec![on(DependencyKind::SyntheticMonitor, " ")]).is_err());
    }
}
