// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The error rules where errors are fingerprinted: each organization's, read
//! again every [`REFRESH_EVERY`] and at once when this node saves them, put
//! on each fact with an error before it is projected.

use std::{collections::HashMap, sync::Arc, time::Duration};

use parking_lot::RwLock;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    domain::apm::{
        ApmCatalogRepository, ApmSpanFact, ErrorIdentity, ErrorRule, ErrorRuleAction,
        ServiceIdentity, ordered_error_rules,
    },
    shared::ids::Id,
};

/// How long a rule saved on another node takes at most to apply here.
const REFRESH_EVERY: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct ErrorRuleBook {
    rules: RwLock<HashMap<Id, Arc<[ErrorRule]>>>,
}

impl ErrorRuleBook {
    /// Groups the fact's error by the first of its organization's rules that
    /// matches it: into the rule's group, or into none.
    pub fn apply(&self, fact: &mut ApmSpanFact) {
        let Some(error) = &fact.error else {
            return;
        };

        let Some(rules) = self.rules.read().get(&fact.org_id).cloned() else {
            return;
        };

        let message = fact
            .exception
            .as_ref()
            .and_then(|exception| exception.message.as_deref());

        let Some(rule) = rules
            .iter()
            .find(|rule| rule.matches(&fact.service, error, message))
        else {
            return;
        };

        fact.error = match rule.action {
            ErrorRuleAction::Merge => Some(merged_identity(rule, &fact.service)),
            ErrorRuleAction::Ignore => None,
        };
    }

    /// Every organization's rules, as read.
    pub fn replace_all(&self, rules: Vec<(Id, ErrorRule)>) {
        let mut by_org = HashMap::<Id, Vec<ErrorRule>>::new();

        for (org_id, rule) in rules {
            by_org.entry(org_id).or_default().push(rule);
        }

        *self.rules.write() = by_org
            .into_iter()
            .map(|(org_id, rules)| (org_id, ordered_error_rules(rules).into()))
            .collect();
    }

    /// One organization's rules, as just saved.
    pub fn replace_org(&self, org_id: &Id, rules: Vec<ErrorRule>) {
        let mut book = self.rules.write();

        if rules.is_empty() {
            book.remove(org_id);
        } else {
            book.insert(org_id.clone(), ordered_error_rules(rules).into());
        }
    }
}

/// The group a merging rule makes of a service's errors: one per rule and
/// service, named by the rule.
pub fn merged_identity(rule: &ErrorRule, service: &ServiceIdentity) -> ErrorIdentity {
    let seed = format!(
        "rule\u{0}{}\u{0}{}\u{0}{}",
        rule.id.as_str(),
        service.namespace,
        service.name
    );

    ErrorIdentity {
        fingerprint: blake3::hash(seed.as_bytes()).to_hex()[..32].to_owned(),
        error_type: rule.name.trim().to_owned(),
        application_frame: None,
        transaction_name: None,
        overflow: false,
    }
}

/// Reads every organization's rules into `book` now and every [`REFRESH_EVERY`]
/// until `cancel`; a failed read keeps the rules read before.
pub fn spawn_refresh(
    book: Arc<ErrorRuleBook>,
    catalog: Arc<dyn ApmCatalogRepository>,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(REFRESH_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tick.tick() => match catalog.list_all_error_rules().await {
                    Ok(rules) => book.replace_all(rules),
                    Err(error) => tracing::warn!(
                        target: "molesignal::app::apm",
                        error = %error,
                        "APM error rules could not be read; keeping the rules read before"
                    ),
                },
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::apm::{
            APM_FACT_SCHEMA_VERSION, ApmOutcome, ApmSpanKind, InstrumentationMetadata,
            SanitizedException, TransactionIdentity, TransactionKind,
        },
        shared::time::TimestampMicros,
    };

    fn rule(id: &str, action: ErrorRuleAction, error_type: &str, position: u32) -> ErrorRule {
        ErrorRule {
            id: Id::from_string(id),
            name: format!("Rule {id}"),
            service_namespace: String::new(),
            service_name: String::new(),
            error_type: error_type.into(),
            message: String::new(),
            frame: String::new(),
            endpoint: String::new(),
            action,
            enabled: true,
            position,
            updated_by: Id::from_string("user"),
            updated_at: TimestampMicros(1),
        }
    }

    fn fact(error_type: &str) -> ApmSpanFact {
        ApmSpanFact {
            schema_version: APM_FACT_SCHEMA_VERSION,
            org_id: Id::from_string("org-1"),
            service: ServiceIdentity::new(Some("shop"), Some("checkout"), Some("prod"), None),
            service_version: None,
            service_instance_id: None,
            instrumentation: InstrumentationMetadata::default(),
            trace_id: "trace".into(),
            span_id: "span".into(),
            parent_span_id: None,
            event_time: TimestampMicros(1),
            duration: 10,
            span_kind: ApmSpanKind::Server,
            outcome: ApmOutcome::Error,
            transaction: Some(TransactionIdentity {
                name: "GET /orders".into(),
                kind: TransactionKind::Http,
            }),
            dependency: None,
            error: Some(ErrorIdentity {
                fingerprint: "original".into(),
                error_type: error_type.into(),
                application_frame: None,
                transaction_name: Some("GET /orders".into()),
                overflow: false,
            }),
            exception: Some(SanitizedException {
                error_type: error_type.into(),
                message: Some("upstream timed out".into()),
                stack_frames: Vec::new(),
            }),
            response_status: None,
            timed_out: false,
            retried: false,
            instance: None,
        }
    }

    #[test]
    fn the_first_rule_that_matches_groups_the_error() {
        let book = ErrorRuleBook::default();

        book.replace_all(vec![
            (
                Id::from_string("org-1"),
                rule("b", ErrorRuleAction::Ignore, "*Timeout*", 1),
            ),
            (
                Id::from_string("org-1"),
                rule("a", ErrorRuleAction::Merge, "*Timeout*", 0),
            ),
        ]);

        let mut merged = fact("ReadTimeoutError");
        book.apply(&mut merged);
        let error = merged.error.expect("merged");

        assert_eq!(error.error_type, "Rule a");
        assert_ne!(error.fingerprint, "original");
        assert_eq!(error.transaction_name, None);

        let mut untouched = fact("KeyError");
        book.apply(&mut untouched);

        assert_eq!(untouched.error.expect("kept").fingerprint, "original");
    }

    #[test]
    fn an_ignoring_rule_leaves_no_group_and_other_organizations_alone() {
        let book = ErrorRuleBook::default();

        book.replace_org(
            &Id::from_string("org-1"),
            vec![rule("a", ErrorRuleAction::Ignore, "*Timeout*", 0)],
        );

        let mut ignored = fact("ReadTimeoutError");
        book.apply(&mut ignored);

        assert!(ignored.error.is_none());

        let mut elsewhere = fact("ReadTimeoutError");
        elsewhere.org_id = Id::from_string("org-2");
        book.apply(&mut elsewhere);

        assert!(elsewhere.error.is_some());

        book.replace_org(&Id::from_string("org-1"), Vec::new());

        let mut cleared = fact("ReadTimeoutError");
        book.apply(&mut cleared);

        assert!(cleared.error.is_some());
    }

    #[test]
    fn a_merged_group_is_one_per_rule_and_service() {
        let merge = rule("a", ErrorRuleAction::Merge, "*", 0);
        let checkout = ServiceIdentity::new(Some("shop"), Some("checkout"), Some("prod"), None);
        let staging = ServiceIdentity::new(Some("shop"), Some("checkout"), Some("staging"), None);
        let cart = ServiceIdentity::new(Some("shop"), Some("cart"), Some("prod"), None);

        assert_eq!(
            merged_identity(&merge, &checkout).fingerprint,
            merged_identity(&merge, &staging).fingerprint,
        );
        assert_ne!(
            merged_identity(&merge, &checkout).fingerprint,
            merged_identity(&merge, &cart).fingerprint,
        );
    }
}
