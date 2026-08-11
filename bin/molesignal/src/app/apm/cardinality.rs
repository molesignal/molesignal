// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Hour-window cardinality admission for bounded APM dimensions.

use std::collections::{HashMap, HashSet};

use crate::{
    config::ApmCardinalitySettings,
    domain::apm::{
        ApmSpanFact, DependencyIdentity, ErrorIdentity, ServiceIdentity, TransactionIdentity,
    },
    shared::ids::Id,
};

const HOUR_MICROS: i64 = 3_600_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardinalityReason {
    ServiceRejected,
    TransactionOverflow,
    DependencyOverflow,
    ErrorOverflow,
    VersionSuppressed,
    InstanceSuppressed,
}

impl CardinalityReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ServiceRejected => "service_rejected",
            Self::TransactionOverflow => "transaction_overflow",
            Self::DependencyOverflow => "dependency_overflow",
            Self::ErrorOverflow => "error_overflow",
            Self::VersionSuppressed => "version_suppressed",
            Self::InstanceSuppressed => "instance_suppressed",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CardinalityDecision {
    pub accepted: bool,
    pub reasons: Vec<CardinalityReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct OrgWindowKey {
    org_id: Id,
    hour_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ServiceWindowKey {
    org_id: Id,
    hour_at: i64,
    service: ServiceIdentity,
}

#[derive(Default)]
struct ServiceWindow {
    transactions: HashSet<TransactionIdentity>,
    dependencies: HashSet<DependencyIdentity>,
    errors: HashSet<ErrorIdentity>,
    versions: HashSet<String>,
    instances: HashSet<String>,
}

pub struct ApmCardinalityLimiter {
    limits: ApmCardinalitySettings,
    org_services: HashMap<OrgWindowKey, HashSet<ServiceIdentity>>,
    service_windows: HashMap<ServiceWindowKey, ServiceWindow>,
    newest_hour: i64,
}

impl ApmCardinalityLimiter {
    pub fn new(limits: ApmCardinalitySettings) -> Self {
        Self {
            limits,
            org_services: HashMap::new(),
            service_windows: HashMap::new(),
            newest_hour: i64::MIN,
        }
    }

    pub fn admit(&mut self, fact: &mut ApmSpanFact) -> CardinalityDecision {
        let hour_at = fact.event_time.0.div_euclid(HOUR_MICROS) * HOUR_MICROS;
        self.prune(hour_at);
        let org_key = OrgWindowKey {
            org_id: fact.org_id.clone(),
            hour_at,
        };
        let services = self.org_services.entry(org_key).or_default();
        if !admit_value(
            services,
            fact.service.clone(),
            self.limits.services_per_org_hour,
        ) {
            return CardinalityDecision {
                accepted: false,
                reasons: vec![CardinalityReason::ServiceRejected],
            };
        }

        let window = self
            .service_windows
            .entry(ServiceWindowKey {
                org_id: fact.org_id.clone(),
                hour_at,
                service: fact.service.clone(),
            })
            .or_default();
        let mut reasons = Vec::new();

        if let Some(transaction) = fact.transaction.as_mut()
            && !admit_value(
                &mut window.transactions,
                transaction.clone(),
                self.limits.transactions_per_service_hour,
            )
        {
            *transaction = TransactionIdentity::other();
            reasons.push(CardinalityReason::TransactionOverflow);
        }
        if let Some(dependency) = fact.dependency.as_mut()
            && !admit_value(
                &mut window.dependencies,
                dependency.clone(),
                self.limits.dependencies_per_service_hour,
            )
        {
            *dependency = DependencyIdentity::other(dependency.category);
            reasons.push(CardinalityReason::DependencyOverflow);
        }
        if let Some(error) = fact.error.as_mut()
            && !admit_value(
                &mut window.errors,
                error.clone(),
                self.limits.error_groups_per_service_hour,
            )
        {
            *error = overflow_error(&fact.org_id, &fact.service);
            reasons.push(CardinalityReason::ErrorOverflow);
        }
        if let Some(version) = fact.service_version.as_ref()
            && !admit_value(
                &mut window.versions,
                version.clone(),
                self.limits.versions_per_service_hour,
            )
        {
            fact.service_version = None;
            reasons.push(CardinalityReason::VersionSuppressed);
        }
        if let Some(instance) = fact.service_instance_id.as_ref()
            && !admit_value(
                &mut window.instances,
                instance.clone(),
                self.limits.instances_per_service_hour,
            )
        {
            fact.service_instance_id = None;
            reasons.push(CardinalityReason::InstanceSuppressed);
        }

        CardinalityDecision {
            accepted: true,
            reasons,
        }
    }

    fn prune(&mut self, observed_hour: i64) {
        self.newest_hour = self.newest_hour.max(observed_hour);
        let keep_from = self.newest_hour.saturating_sub(2 * HOUR_MICROS);
        self.org_services.retain(|key, _| key.hour_at >= keep_from);
        self.service_windows
            .retain(|key, _| key.hour_at >= keep_from);
    }
}

fn admit_value<T>(values: &mut HashSet<T>, value: T, limit: usize) -> bool
where
    T: Eq + std::hash::Hash,
{
    if values.contains(&value) {
        return true;
    }
    if values.len() >= limit {
        return false;
    }
    values.insert(value);
    true
}

fn overflow_error(org_id: &Id, service: &ServiceIdentity) -> ErrorIdentity {
    let seed = format!("{}\u{0}{}\u{0}__other__", org_id, service.stable_key());
    ErrorIdentity {
        fingerprint: blake3::hash(seed.as_bytes()).to_hex()[..32].to_owned(),
        error_type: crate::domain::apm::OTHER_DIMENSION.to_owned(),
        application_frame: None,
        transaction_name: None,
        overflow: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::apm::{
            APM_FACT_SCHEMA_VERSION, ApmOutcome, ApmSpanKind, DependencyCategory,
            InstrumentationMetadata, TransactionKind,
        },
        shared::time::TimestampMicros,
    };

    fn fact(service: &str) -> ApmSpanFact {
        ApmSpanFact {
            schema_version: APM_FACT_SCHEMA_VERSION,
            org_id: Id::from_string("org-1"),
            service: ServiceIdentity::new(None, Some(service), None, None),
            service_version: Some("v1".into()),
            service_instance_id: Some("i1".into()),
            instrumentation: InstrumentationMetadata::default(),
            trace_id: "trace".into(),
            span_id: "span".into(),
            parent_span_id: None,
            event_time: TimestampMicros(1),
            duration_micros: 10,
            span_kind: ApmSpanKind::Server,
            outcome: ApmOutcome::Success,
            transaction: Some(TransactionIdentity {
                name: "GET /one".into(),
                kind: TransactionKind::Http,
            }),
            dependency: Some(DependencyIdentity {
                category: DependencyCategory::Service,
                target: "one".into(),
                operation: None,
            }),
            error: None,
            exception: None,
        }
    }

    fn limits() -> ApmCardinalitySettings {
        ApmCardinalitySettings {
            services_per_org_hour: 1,
            transactions_per_service_hour: 1,
            dependencies_per_service_hour: 1,
            error_groups_per_service_hour: 1,
            versions_per_service_hour: 1,
            instances_per_service_hour: 1,
        }
    }

    #[test]
    fn rejects_new_service_but_keeps_existing_service() {
        let mut limiter = ApmCardinalityLimiter::new(limits());
        assert!(limiter.admit(&mut fact("a")).accepted);
        assert!(limiter.admit(&mut fact("a")).accepted);
        let decision = limiter.admit(&mut fact("b"));
        assert!(!decision.accepted);
        assert_eq!(decision.reasons, vec![CardinalityReason::ServiceRejected]);
    }

    #[test]
    fn overflows_detail_dimensions_and_suppresses_version_instance() {
        let mut limiter = ApmCardinalityLimiter::new(limits());
        assert!(limiter.admit(&mut fact("a")).accepted);
        let mut overflow = fact("a");
        overflow.transaction.as_mut().unwrap().name = "GET /two".into();
        overflow.dependency.as_mut().unwrap().target = "two".into();
        overflow.service_version = Some("v2".into());
        overflow.service_instance_id = Some("i2".into());
        let decision = limiter.admit(&mut overflow);
        assert!(decision.accepted);
        assert_eq!(
            overflow.transaction.as_ref().unwrap().name,
            crate::domain::apm::OTHER_DIMENSION
        );
        assert_eq!(
            overflow.dependency.as_ref().unwrap().target,
            crate::domain::apm::OTHER_DIMENSION
        );
        assert!(overflow.service_version.is_none());
        assert!(overflow.service_instance_id.is_none());
    }

    #[test]
    fn error_overflow_uses_one_explicit_group() {
        let mut limiter = ApmCardinalityLimiter::new(limits());
        let mut first = fact("a");
        first.error = Some(ErrorIdentity {
            fingerprint: "first".into(),
            error_type: "First".into(),
            application_frame: None,
            transaction_name: None,
            overflow: false,
        });
        assert!(limiter.admit(&mut first).accepted);
        let mut second = fact("a");
        second.error = Some(ErrorIdentity {
            fingerprint: "second".into(),
            error_type: "Second".into(),
            application_frame: None,
            transaction_name: None,
            overflow: false,
        });
        let decision = limiter.admit(&mut second);
        assert!(second.error.as_ref().unwrap().overflow);
        assert!(decision.reasons.contains(&CardinalityReason::ErrorOverflow));
    }
}
