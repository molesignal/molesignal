// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Names that should agree across the data: a service named one way in every signal, an
//! environment spelled one way, a metric that says what unit it counts in.

use std::collections::{BTreeMap, BTreeSet};

/// Where a service name was seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Signal {
    Logs,
    Metrics,
    Traces,
}

impl Signal {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Logs => "logs",
            Self::Metrics => "metrics",
            Self::Traces => "traces",
        }
    }
}

/// Spellings of one environment, each a list of what it is written as.
const ENVIRONMENT_MEANINGS: &[&[&str]] = &[
    &["prod", "production", "prd", "live"],
    &[
        "staging",
        "stage",
        "stg",
        "preprod",
        "pre-prod",
        "pre-production",
    ],
    &["dev", "development", "develop", "devel"],
    &["test", "testing", "tst"],
];

/// A name with case and separators set aside: `Order-API`, `order_api` and `order.api` read the
/// same, `order-api` and `order-service` do not.
pub(super) fn loose_name(name: &str) -> String {
    let mut loose = String::new();

    for character in name.trim().chars().flat_map(char::to_lowercase) {
        if matches!(character, '-' | '_' | '.' | ' ' | '/') {
            if !loose.ends_with('-') && !loose.is_empty() {
                loose.push('-');
            }
        } else {
            loose.push(character);
        }
    }

    loose.trim_end_matches('-').to_string()
}

/// A service written in more than one way, with the signals each spelling was seen in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NearMiss {
    pub(super) spellings: Vec<(String, Vec<Signal>)>,
}

/// The services written in more than one way: names that differ only in case or separators.
pub(super) fn near_misses(seen: &BTreeMap<String, BTreeSet<Signal>>) -> Vec<NearMiss> {
    let mut groups = BTreeMap::<String, Vec<(&String, &BTreeSet<Signal>)>>::new();

    for (name, signals) in seen {
        groups
            .entry(loose_name(name))
            .or_default()
            .push((name, signals));
    }

    groups
        .into_values()
        .filter(|spellings| spellings.len() > 1)
        .map(|spellings| NearMiss {
            spellings: spellings
                .into_iter()
                .map(|(name, signals)| (name.clone(), signals.iter().copied().collect()))
                .collect(),
        })
        .collect()
}

/// The services seen in one signal only, when more than one signal names services: by signal,
/// each service under the first of its spellings.
pub(super) fn only_in_one_signal(
    seen: &BTreeMap<String, BTreeSet<Signal>>,
) -> BTreeMap<Signal, Vec<String>> {
    let naming = seen
        .values()
        .flat_map(|signals| signals.iter().copied())
        .collect::<BTreeSet<_>>();

    if naming.len() < 2 {
        return BTreeMap::new();
    }

    let mut by_loose = BTreeMap::<String, (String, BTreeSet<Signal>)>::new();

    for (name, signals) in seen {
        let entry = by_loose
            .entry(loose_name(name))
            .or_insert_with(|| (name.clone(), BTreeSet::new()));

        entry.1.extend(signals.iter().copied());
    }

    let mut only = BTreeMap::<Signal, Vec<String>>::new();

    for (name, signals) in by_loose.into_values() {
        let signals = signals.into_iter().collect::<Vec<_>>();

        if let [signal] = signals.as_slice() {
            only.entry(*signal).or_default().push(name);
        }
    }

    only
}

/// What one environment value means: its place in [`ENVIRONMENT_MEANINGS`], or else the value
/// with case and separators set aside.
fn environment_meaning(value: &str) -> String {
    let lower = value.trim().to_lowercase();

    ENVIRONMENT_MEANINGS
        .iter()
        .position(|spellings| spellings.contains(&lower.as_str()))
        .map_or_else(|| loose_name(&lower), |index| format!("#{index}"))
}

/// The environments written in more than one way, each with its spellings and how often each
/// was seen, the most frequent first.
pub(super) fn environment_spellings(values: &BTreeMap<String, u64>) -> Vec<Vec<(String, u64)>> {
    let mut groups = BTreeMap::<String, Vec<(String, u64)>>::new();

    for (value, count) in values {
        if value.trim().is_empty() {
            continue;
        }

        groups
            .entry(environment_meaning(value))
            .or_default()
            .push((value.clone(), *count));
    }

    groups
        .into_values()
        .filter(|spellings| spellings.len() > 1)
        .map(|mut spellings| {
            spellings
                .sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
            spellings
        })
        .collect()
}

/// What a metric's name lacks to say what it counts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum UnitGap {
    /// A duration without its unit: `_seconds` by convention.
    Duration,
    /// A size without its unit: `_bytes` by convention.
    Size,
    /// A counter without `_total`.
    CounterTotal,
}

const HISTOGRAM_PARTS: &[&str] = &["bucket", "count", "sum", "created"];
const DURATION_WORDS: &[&str] = &[
    "latency", "duration", "elapsed", "wait", "delay", "uptime", "lag", "rtt", "time",
];
const TIME_UNITS: &[&str] = &[
    "seconds",
    "second",
    "secs",
    "sec",
    "milliseconds",
    "millisecond",
    "millis",
    "ms",
    "microseconds",
    "microsecond",
    "us",
    "nanoseconds",
    "nanosecond",
    "ns",
    "minutes",
    "hours",
    "days",
    "timestamp",
    "ratio",
    "percent",
    "total",
    "count",
];
const SIZE_WORDS: &[&str] = &["size", "memory", "mem", "heap", "disk", "payload", "rss"];
const SIZE_UNITS: &[&str] = &[
    "bytes",
    "byte",
    "kilobytes",
    "megabytes",
    "gigabytes",
    "kb",
    "mb",
    "gb",
    "kib",
    "mib",
    "gib",
    "bits",
    "ratio",
    "percent",
    "pages",
    "blocks",
    "objects",
    "entries",
    "items",
    "count",
    "total",
    "utilization",
    "usage",
];

/// What the name of a metric of kind `kind` (`counter` and the like, when known) lacks to say
/// what it counts in. Histogram parts (`_bucket`, `_count`, `_sum`) are read without the part.
pub(super) fn unit_gap(metric: &str, kind: Option<&str>) -> Option<UnitGap> {
    let lower = metric.to_lowercase();

    let mut words = lower
        .split(['_', '.', ':'])
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();

    if words.len() > 1
        && words
            .last()
            .is_some_and(|last| HISTOGRAM_PARTS.contains(last))
    {
        words.pop();
    }

    let has = |list: &[&str]| words.iter().any(|word| list.contains(word));

    if has(DURATION_WORDS) && !has(TIME_UNITS) {
        return Some(UnitGap::Duration);
    }

    if has(SIZE_WORDS) && !has(SIZE_UNITS) {
        return Some(UnitGap::Size);
    }

    let counter = kind.is_some_and(|kind| kind.eq_ignore_ascii_case("counter"));

    (counter && !lower.ends_with("_total")).then_some(UnitGap::CounterTotal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(names: &[(&str, &[Signal])]) -> BTreeMap<String, BTreeSet<Signal>> {
        names
            .iter()
            .map(|(name, signals)| ((*name).to_string(), signals.iter().copied().collect()))
            .collect()
    }

    #[test]
    fn names_that_differ_in_case_or_separators_are_one_name_and_others_are_not() {
        assert_eq!(loose_name("Order-API"), "order-api");
        assert_eq!(loose_name("order_api"), "order-api");
        assert_eq!(loose_name("order..api "), "order-api");
        assert_ne!(loose_name("order-api"), loose_name("order_service"));
    }

    #[test]
    fn a_service_written_two_ways_is_a_near_miss() {
        let found = near_misses(&seen(&[
            ("order-api", &[Signal::Logs, Signal::Traces]),
            ("order_api", &[Signal::Metrics]),
            ("order_service", &[Signal::Metrics]),
            ("payments", &[Signal::Logs]),
        ]));

        assert_eq!(
            found,
            [NearMiss {
                spellings: vec![
                    ("order-api".to_string(), vec![Signal::Logs, Signal::Traces]),
                    ("order_api".to_string(), vec![Signal::Metrics]),
                ],
            }]
        );
    }

    #[test]
    fn a_service_in_one_signal_only_is_named_under_that_signal() {
        let found = only_in_one_signal(&seen(&[
            ("checkout", &[Signal::Logs, Signal::Traces]),
            ("Checkout", &[Signal::Metrics]),
            ("batch-import", &[Signal::Logs]),
            ("edge", &[Signal::Traces]),
        ]));

        assert_eq!(found[&Signal::Logs], ["batch-import"]);
        assert_eq!(found[&Signal::Traces], ["edge"]);
        assert!(!found.contains_key(&Signal::Metrics));

        // One signal alone says nothing about the others.
        assert!(only_in_one_signal(&seen(&[("a", &[Signal::Logs])])).is_empty());
    }

    #[test]
    fn spellings_of_one_environment_are_grouped_the_most_frequent_first() {
        let values = BTreeMap::from([
            ("prod".to_string(), 900),
            ("production".to_string(), 40),
            ("PROD".to_string(), 3),
            ("staging".to_string(), 10),
            ("prod-eu".to_string(), 5),
        ]);

        let groups = environment_spellings(&values);

        assert_eq!(
            groups,
            [vec![
                ("prod".to_string(), 900),
                ("production".to_string(), 40),
                ("PROD".to_string(), 3),
            ]]
        );
    }

    #[test]
    fn a_metric_says_its_unit_or_is_named_for_lacking_one() {
        assert_eq!(unit_gap("http_server_duration_seconds_bucket", None), None);
        assert_eq!(
            unit_gap("http_server_duration_bucket", None),
            Some(UnitGap::Duration)
        );
        assert_eq!(unit_gap("db_query_latency_ms", None), None);
        assert_eq!(unit_gap("process_start_time_seconds", None), None);
        assert_eq!(unit_gap("jvm_heap_used", None), Some(UnitGap::Size));
        assert_eq!(unit_gap("jvm_heap_used_bytes", None), None);
        assert_eq!(
            unit_gap("http_requests", Some("counter")),
            Some(UnitGap::CounterTotal)
        );
        assert_eq!(unit_gap("http_requests_total", Some("counter")), None);
        assert_eq!(unit_gap("queue_depth", Some("gauge")), None);
    }
}
