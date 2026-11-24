// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Who the incident touched in business terms: for each business key the Spans carry, the values
//! whose requests to the incident's services failed most around the time it began, the values
//! marked important told as such.

use domain::{
    alerting::rca_evidence::{EvidenceKind, EvidenceLink, EvidenceSource, RcaEvidence},
    business_context::BusinessContextRepository,
    stream::StreamRepository,
};
use kernel::{Result, time::TimeRange};

use super::{super::incident_services, Scope};
use crate::app::{
    business_impact::{ImpactQuery, KeyImpact, ValueImpact, business_impact},
    query::QueryService,
};

/// The most values named in a piece.
const NAMED_VALUES: usize = 3;

pub(super) async fn impact(
    query: &QueryService,
    streams: &dyn StreamRepository,
    business: &dyn BusinessContextRepository,
    scope: &Scope<'_>,
) -> Result<Vec<RcaEvidence>> {
    let org_id = &scope.incident.org_id;
    let context = business.business_context(org_id).await?;

    if context.keys.is_empty() {
        return Ok(Vec::new());
    }

    // The incident's own services: those it is about. Without any, those evidence is read for.
    let mut services = incident_services(scope.incident)
        .into_iter()
        .collect::<Vec<_>>();
    services.sort();

    if services.is_empty() {
        services.clone_from(&scope.services);
    }

    let asked = ImpactQuery {
        org_id,
        context: &context,
        services: &services,
        window: scope.window,
    };

    let found = business_impact(query, streams, &asked).await?;

    Ok(found
        .iter()
        .filter_map(|key| told(key, scope.window))
        .collect())
}

/// A key whose requests failed, as evidence: the values that failed most named, the important
/// ones first.
fn told(key: &KeyImpact, window: TimeRange) -> Option<RcaEvidence> {
    let mut failing = key
        .values
        .iter()
        .filter(|value| value.errors > 0)
        .collect::<Vec<_>>();

    if failing.is_empty() {
        return None;
    }

    failing.sort_by(|left, right| {
        right
            .important
            .cmp(&left.important)
            .then_with(|| right.errors.cmp(&left.errors))
    });

    let named = failing
        .iter()
        .take(NAMED_VALUES)
        .map(|value| {
            if value.important {
                format!("{} (important)", value.value)
            } else {
                value.value.clone()
            }
        })
        .collect::<Vec<_>>();

    let mut piece = RcaEvidence::new(EvidenceKind::BusinessImpact, EvidenceSource::Traces);

    piece.subject = Some(key.label.clone());
    piece.detail = Some(named.join(", "));

    let sum = |of: fn(&&ValueImpact) -> u64| failing.iter().map(of).sum::<u64>() as f64;

    piece
        .figures
        .insert("errors".into(), sum(|value| value.errors));
    piece
        .figures
        .insert("requests".into(), sum(|value| value.requests));
    piece.figures.insert("values".into(), failing.len() as f64);
    piece.figures.insert(
        "important".into(),
        failing.iter().filter(|value| value.important).count() as f64,
    );

    piece.link = Some(EvidenceLink {
        from: Some(window.start),
        to: Some(window.end),
        ..EvidenceLink::default()
    });

    Some(piece)
}

#[cfg(test)]
mod tests {
    use kernel::time::TimestampMicros;

    use super::*;

    fn value(name: &str, errors: u64, important: bool) -> ValueImpact {
        ValueImpact {
            value: name.into(),
            requests: errors * 10 + 5,
            errors,
            important,
        }
    }

    #[test]
    fn the_failed_values_are_named_the_important_first() {
        let window = TimeRange::new(TimestampMicros(1), TimestampMicros(2));

        let key = KeyImpact {
            key: "tenant".into(),
            label: "Tenant".into(),
            field: "tenant.id".into(),
            values: vec![
                value("globex", 40, false),
                value("initech", 0, false),
                value("acme", 3, true),
            ],
            more: false,
        };

        let piece = told(&key, window).expect("a piece");

        assert_eq!(piece.subject.as_deref(), Some("Tenant"));
        assert_eq!(piece.detail.as_deref(), Some("acme (important), globex"));
        assert_eq!(piece.figures.get("errors"), Some(&43.0));
        assert_eq!(piece.figures.get("values"), Some(&2.0));
        assert_eq!(piece.figures.get("important"), Some(&1.0));

        let calm = KeyImpact {
            values: vec![value("initech", 0, false)],
            ..key
        };
        assert_eq!(told(&calm, window), None);
    }
}
