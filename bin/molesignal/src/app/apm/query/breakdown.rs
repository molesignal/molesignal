// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Where errors happened: how many occurrences each version, environment and
//! language had, read from the error buckets and the languages the services
//! were seen running.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;

use crate::domain::apm::{
    BucketDimension, MergedBucket, OTHER_DIMENSION, ServiceIdentity, ServiceObservation,
};

/// The most values a breakdown names; the rest are counted together.
const MOST_VALUES: usize = 8;

#[derive(Debug, Clone, Default, Serialize)]
pub struct ErrorBreakdown {
    pub versions: Vec<ErrorShare>,
    pub environments: Vec<ErrorShare>,
    pub languages: Vec<ErrorShare>,
}

/// Occurrences of one value: empty when the spans did not say it,
/// [`OTHER_DIMENSION`] for the values past the most named.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorShare {
    pub value: String,
    pub count: u64,
}

/// The breakdown of the error buckets whose group is among `fingerprints`,
/// or of every error bucket without them.
pub(super) fn error_breakdown(
    buckets: &[MergedBucket],
    catalog: &[ServiceObservation],
    fingerprints: Option<&HashSet<&str>>,
) -> ErrorBreakdown {
    let languages = catalog
        .iter()
        .map(|observation| {
            (
                &observation.service,
                observation.runtime_language.as_deref().unwrap_or_default(),
            )
        })
        .collect::<HashMap<&ServiceIdentity, &str>>();

    let mut versions = BTreeMap::<&str, u64>::new();
    let mut environments = BTreeMap::<&str, u64>::new();
    let mut by_language = BTreeMap::<&str, u64>::new();

    for bucket in buckets {
        let BucketDimension::Error {
            service,
            version,
            error,
        } = &bucket.dimension
        else {
            continue;
        };

        if fingerprints.is_some_and(|kept| !kept.contains(error.fingerprint.as_str())) {
            continue;
        }

        let count = bucket.measurements.request_count;

        if count == 0 {
            continue;
        }

        *versions
            .entry(version.as_deref().unwrap_or_default())
            .or_default() += count;
        *environments
            .entry(service.environment.as_str())
            .or_default() += count;
        *by_language
            .entry(languages.get(service).copied().unwrap_or_default())
            .or_default() += count;
    }

    ErrorBreakdown {
        versions: shares(versions),
        environments: shares(environments),
        languages: shares(by_language),
    }
}

/// The values by count, the largest first, past [`MOST_VALUES`] counted together.
fn shares(counts: BTreeMap<&str, u64>) -> Vec<ErrorShare> {
    let mut shares = counts
        .into_iter()
        .map(|(value, count)| ErrorShare {
            value: value.to_owned(),
            count,
        })
        .collect::<Vec<_>>();

    shares.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.value.cmp(&right.value))
    });

    if shares.len() > MOST_VALUES {
        let rest = shares
            .split_off(MOST_VALUES - 1)
            .iter()
            .map(|share| share.count)
            .sum();

        shares.push(ErrorShare {
            value: OTHER_DIMENSION.to_owned(),
            count: rest,
        });
    }

    shares
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_past_the_most_named_are_counted_together() {
        let counts = (0..10)
            .map(|index| {
                (
                    ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"][index],
                    10 - index as u64,
                )
            })
            .collect::<BTreeMap<_, _>>();

        let shares = shares(counts);

        assert_eq!(shares.len(), MOST_VALUES);
        assert_eq!(
            shares[0],
            ErrorShare {
                value: "a".into(),
                count: 10
            }
        );
        assert_eq!(
            shares.last(),
            Some(&ErrorShare {
                value: OTHER_DIMENSION.into(),
                count: 3 + 2 + 1,
            }),
        );
    }
}
