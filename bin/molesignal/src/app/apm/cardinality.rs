// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Hour-window cardinality admission for bounded APM dimensions.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use parking_lot::RwLock;

use crate::{
    domain::apm::{
        ApmLimits, ApmSpanFact, DependencyCategory, DependencyIdentity, ErrorIdentity,
        OTHER_DIMENSION, ServiceIdentity, TransactionIdentity,
    },
    shared::ids::Id,
};

const HOUR_MICROS: i64 = 3_600_000_000;

/// The limits a limiter keeps to, a platform setting changed while it runs.
pub type ApmLimitsHandle = Arc<RwLock<ApmLimits>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardinalityReason {
    ServiceRejected,
    TransactionOverflow,
    DependencyOverflow,
    OperationOverflow,
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
            Self::OperationOverflow => "operation_overflow",
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
    /// The targets called, and the operations called on each.
    dependencies: HashSet<(DependencyCategory, String)>,
    operations: HashMap<(DependencyCategory, String), HashSet<String>>,
    errors: HashSet<ErrorIdentity>,
    versions: HashSet<String>,
    instances: HashSet<String>,
}

pub struct ApmCardinalityLimiter {
    limits: ApmLimitsHandle,
    org_services: HashMap<OrgWindowKey, HashSet<ServiceIdentity>>,
    service_windows: HashMap<ServiceWindowKey, ServiceWindow>,
    newest_hour: i64,
}

impl ApmCardinalityLimiter {
    pub fn new(limits: ApmLimitsHandle) -> Self {
        Self {
            limits,
            org_services: HashMap::new(),
            service_windows: HashMap::new(),
            newest_hour: i64::MIN,
        }
    }

    pub fn admit(&mut self, fact: &mut ApmSpanFact) -> CardinalityDecision {
        let limits = *self.limits.read();
        let hour_at = fact.event_time.0.div_euclid(HOUR_MICROS) * HOUR_MICROS;
        self.prune(hour_at);

        let org_key = OrgWindowKey {
            org_id: fact.org_id.clone(),
            hour_at,
        };

        let services = self.org_services.entry(org_key).or_default();

        if !admit_value(services, fact.service.clone(), limits.services_per_org_hour) {
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
                limits.transactions_per_service_hour,
            )
        {
            *transaction = TransactionIdentity::other();
            reasons.push(CardinalityReason::TransactionOverflow);
        }

        if let Some(dependency) = fact.dependency.as_mut() {
            let target = (dependency.category, dependency.target.clone());

            if !admit_value(
                &mut window.dependencies,
                target.clone(),
                limits.dependencies_per_service_hour,
            ) {
                *dependency = DependencyIdentity::other(dependency.category);
                reasons.push(CardinalityReason::DependencyOverflow);
            } else if let Some(operation) = dependency.operation.as_mut()
                && !admit_value(
                    window.operations.entry(target).or_default(),
                    operation.clone(),
                    limits.operations_per_dependency_hour,
                )
            {
                *operation = OTHER_DIMENSION.to_owned();
                reasons.push(CardinalityReason::OperationOverflow);
            }
        }

        if let Some(error) = fact.error.as_mut()
            && !admit_value(
                &mut window.errors,
                error.clone(),
                limits.error_groups_per_service_hour,
            )
        {
            *error = overflow_error(&fact.org_id, &fact.service);
            reasons.push(CardinalityReason::ErrorOverflow);
        }

        if let Some(version) = fact.service_version.as_ref()
            && !admit_value(
                &mut window.versions,
                version.clone(),
                limits.versions_per_service_hour,
            )
        {
            fact.service_version = None;
            reasons.push(CardinalityReason::VersionSuppressed);
        }

        // Past the limit an instance is counted in the overflow row, so the
        // instances still add up to the service.
        if let Some(instance) = fact.service_instance_id.as_ref()
            && !admit_value(
                &mut window.instances,
                instance.clone(),
                limits.instances_per_service_hour,
            )
        {
            fact.service_instance_id = Some(OTHER_DIMENSION.to_owned());
            reasons.push(CardinalityReason::InstanceSuppressed);
        }

        // A look at an instance told apart by its pod or host counts against the same limit.
        if let Some(sighting) = fact.instance.as_ref()
            && !admit_value(
                &mut window.instances,
                sighting.instance_id.clone(),
                limits.instances_per_service_hour,
            )
        {
            fact.instance = None;
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
        error_type: OTHER_DIMENSION.to_owned(),
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
            duration: 10,
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
            response_status: None,
            timed_out: false,
            retried: false,
            instance: None,
        }
    }

    fn limits() -> ApmLimitsHandle {
        Arc::new(RwLock::new(ApmLimits {
            services_per_org_hour: 1,
            transactions_per_service_hour: 1,
            dependencies_per_service_hour: 1,
            operations_per_dependency_hour: 32,
            error_groups_per_service_hour: 1,
            versions_per_service_hour: 1,
            instances_per_service_hour: 1,
        }))
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
        assert_eq!(
            overflow.service_instance_id.as_deref(),
            Some(crate::domain::apm::OTHER_DIMENSION)
        );
    }

    #[test]
    fn a_target_keeps_its_name_past_its_operation_limit() {
        let mut limiter = ApmCardinalityLimiter::new(limits());
        let with_operation = |operation: &str| {
            let mut value = fact("a");
            value.dependency.as_mut().unwrap().operation = Some(operation.into());
            value
        };

        for index in 0..32 {
            let mut admitted = with_operation(&format!("SELECT {index}"));
            assert!(limiter.admit(&mut admitted).reasons.is_empty());
        }

        let mut past = with_operation("SELECT past");
        let decision = limiter.admit(&mut past);
        let dependency = past.dependency.unwrap();

        assert_eq!(decision.reasons, vec![CardinalityReason::OperationOverflow]);
        assert_eq!(dependency.target, "one");
        assert_eq!(dependency.operation.as_deref(), Some(OTHER_DIMENSION));
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
