// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The instances a service runs as, gathered from the looks some facts carry,
//! and the kinds of entry a service serves, gathered from every entry span.

use super::ApmAggregator;
use crate::domain::apm::{ApmSpanFact, InstanceObservation, TransactionKind};

pub(super) struct InstanceState {
    pub(super) observation: InstanceObservation,
    pub(super) dirty: bool,
}

impl ApmAggregator {
    /// Takes in a fact's look at its instance, when it carries one.
    pub(super) fn observe_instance(&mut self, fact: &ApmSpanFact) {
        let Some(sighting) = fact.instance.as_deref() else {
            return;
        };

        let state = self
            .instance_records
            .entry((
                fact.org_id.clone(),
                fact.service.clone(),
                sighting.instance_id.clone(),
            ))
            .or_insert_with(|| InstanceState {
                observation: InstanceObservation {
                    org_id: fact.org_id.clone(),
                    service: fact.service.clone(),
                    instance_id: sighting.instance_id.clone(),
                    version: None,
                    first_seen_at: fact.event_time,
                    last_seen_at: fact.event_time,
                    attributes: Default::default(),
                },
                dirty: true,
            });

        state
            .observation
            .absorb(sighting, fact.service_version.as_deref(), fact.event_time);
        state.dirty = true;
    }
}

/// Adds the kind of entry a fact served to the kinds a service is seen
/// serving; the overflow row is not a kind.
pub(super) fn note_entry_kind(kinds: &mut Vec<TransactionKind>, fact: &ApmSpanFact) {
    let Some(kind) = fact
        .transaction
        .as_ref()
        .map(|transaction| transaction.kind)
    else {
        return;
    };

    if kind != TransactionKind::Other && !kinds.contains(&kind) {
        kinds.push(kind);
        kinds.sort();
    }
}
