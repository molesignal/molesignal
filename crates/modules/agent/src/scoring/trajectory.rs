// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What an answer's tool calls say about the data it had to work with, apart from how good the
//! answer is.
//!
//! Read from the steps of one answer: which streams were queried how often and how the queries came
//! back, how often the structure was looked up, how many calls it took before data first came back,
//! and how often raw rows were pulled. Three kinds of gap are named from them:
//!
//! - [`GapKind::DataMap`]: the same stream queried [`REPEATED_QUERIES`] times or more with mostly
//!   empty or failed results, or the structure looked up [`STRUCTURE_LOOKUPS`] times or more:
//!   nothing told the model where the data is or how it is shaped.
//! - [`GapKind::FieldAlignment`]: a service had data in metrics or APM, but a log or trace query
//!   naming the same service came back empty: the signals name it differently.
//! - [`GapKind::FeatureTools`]: raw logs or raw series pulled [`RAW_PULLS`] times or more with many
//!   rows each, without `analyze_log_patterns` or `analyze_metric_trend`.
//!
//! None of it changes a score: it points at the data, not at the answer. Results are read
//! defensively, since they may be cut at the inline limit of the history or be error text.

mod calls;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use self::calls::{Outcome, Role, Signal, ToolUse};
use crate::runs::TranscriptStep;

/// Queries of one stream from this many on, mostly empty or failed, say the model did not know
/// where the data is.
pub const REPEATED_QUERIES: usize = 3;
/// Looking up the structure this many times or more says the same.
pub const STRUCTURE_LOOKUPS: usize = 3;
/// Raw pulls of many rows from this many on, without a tool that sums them up.
pub const RAW_PULLS: usize = 2;
/// A pull returns many rows from this many on, or when its result was cut for length.
pub const MANY_ROWS: usize = 500;

const LOG_PATTERNS_TOOL: &str = "analyze_log_patterns";
const METRIC_TREND_TOOL: &str = "analyze_metric_trend";

/// What a gap says is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapKind {
    /// Where the data is and how it is shaped.
    DataMap,
    /// One name for a service across logs, metrics and traces.
    FieldAlignment,
    /// A tool that sums the data up before the model reads it.
    FeatureTools,
}

impl GapKind {
    pub const ALL: [Self; 3] = [Self::DataMap, Self::FieldAlignment, Self::FeatureTools];
}

/// The rule that found a gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapRule {
    RepeatedQueries,
    StructureLookups,
    ServiceMismatch,
    RawLogPulls,
    RawMetricPulls,
}

impl GapRule {
    pub const fn kind(self) -> GapKind {
        match self {
            Self::RepeatedQueries | Self::StructureLookups => GapKind::DataMap,
            Self::ServiceMismatch => GapKind::FieldAlignment,
            Self::RawLogPulls | Self::RawMetricPulls => GapKind::FeatureTools,
        }
    }
}

/// One gap of one answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Gap {
    pub kind: GapKind,
    pub rule: GapRule,
    /// The stream, metric or `traces` it is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
    /// The service named differently (field alignment only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    /// The calls the rule counted: queries, lookups or pulls.
    pub count: usize,
    /// Of those, the ones that came back empty or failed.
    #[serde(default)]
    pub misses: usize,
}

/// The queries of one tool on one stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamQueries {
    /// The stream, the metric of a PromQL query, or `traces`.
    pub stream: String,
    pub tool: String,
    pub count: usize,
    pub empty: usize,
    pub failed: usize,
}

/// What the tool calls of one answer say about its data.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trajectory {
    pub calls: usize,
    pub queries_by_stream: Vec<StreamQueries>,
    pub schema_lookups: usize,
    pub empty_results: usize,
    pub failed_queries: usize,
    /// Calls made before the first one that brought data back; absent when none did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calls_before_first_data: Option<usize>,
    pub raw_log_pulls: usize,
    pub feature_tools_used: Vec<String>,
    pub gaps: Vec<Gap>,
}

impl Trajectory {
    pub fn has(&self, kind: GapKind) -> bool {
        self.gaps.iter().any(|gap| gap.kind == kind)
    }
}

/// What happened on one stream: queries counted with their misses, pulls of many rows.
#[derive(Default)]
struct Tally {
    by_tool: BTreeMap<(String, String), StreamQueries>,
    lookups: BTreeMap<String, usize>,
    log_pulls: BTreeMap<String, usize>,
    metric_pulls: BTreeMap<String, usize>,
    features: BTreeSet<String>,
}

/// The trajectory of an answer, or `None` when it called no tool.
pub fn analyze(steps: &[TranscriptStep]) -> Option<Trajectory> {
    let uses = steps
        .iter()
        .flat_map(|step| step.calls.iter())
        .map(ToolUse::of)
        .collect::<Vec<_>>();

    if uses.is_empty() {
        return None;
    }

    let mut trajectory = Trajectory {
        calls: uses.len(),
        ..Trajectory::default()
    };
    let mut tally = Tally::default();

    for (index, used) in uses.iter().enumerate() {
        count(&mut trajectory, &mut tally, index, used);
    }

    trajectory.queries_by_stream = tally.by_tool.values().cloned().collect();
    trajectory.feature_tools_used = tally.features.iter().cloned().collect();
    trajectory.raw_log_pulls = tally.log_pulls.values().sum();

    let mut gaps = repeated_queries(&trajectory.queries_by_stream);
    gaps.extend(structure_lookups(trajectory.schema_lookups, &tally.lookups));
    gaps.extend(service_mismatches(&uses));
    gaps.extend(raw_pulls(&tally));
    trajectory.gaps = gaps;

    Some(trajectory)
}

fn count(trajectory: &mut Trajectory, tally: &mut Tally, index: usize, used: &ToolUse) {
    match &used.role {
        Role::Read { signal, target } => {
            if used.outcome.is_data() && trajectory.calls_before_first_data.is_none() {
                trajectory.calls_before_first_data = Some(index);
            }

            // APM is read by service, not by stream: it counts for data and for the services it
            // names, not as queries of a stream.
            if *signal == Signal::Apm {
                return;
            }

            let entry = tally
                .by_tool
                .entry((target.clone(), used.name.clone()))
                .or_insert_with(|| StreamQueries {
                    stream: target.clone(),
                    tool: used.name.clone(),
                    count: 0,
                    empty: 0,
                    failed: 0,
                });

            entry.count += 1;

            match used.outcome {
                Outcome::Failed => {
                    entry.failed += 1;
                    trajectory.failed_queries += 1;
                }
                Outcome::Empty => {
                    entry.empty += 1;
                    trajectory.empty_results += 1;
                }
                Outcome::Data { .. } | Outcome::Unknown => {}
            }

            if used.raw && used.outcome.many_rows() {
                let pulls = match signal {
                    Signal::Logs => Some(&mut tally.log_pulls),
                    Signal::Metrics => Some(&mut tally.metric_pulls),
                    Signal::Traces | Signal::Apm => None,
                };

                if let Some(pulls) = pulls {
                    *pulls.entry(target.clone()).or_default() += 1;
                }
            }
        }
        Role::Lookup { stream } => {
            trajectory.schema_lookups += 1;

            if let Some(stream) = stream {
                *tally.lookups.entry(stream.clone()).or_default() += 1;
            }
        }
        Role::Feature => {
            tally.features.insert(used.name.clone());
        }
        Role::Other => {}
    }
}

/// Streams queried again and again, mostly for nothing.
fn repeated_queries(queries: &[StreamQueries]) -> Vec<Gap> {
    let mut per_stream: BTreeMap<&str, (usize, usize)> = BTreeMap::new();

    for query in queries {
        let entry = per_stream.entry(query.stream.as_str()).or_default();
        entry.0 += query.count;
        entry.1 += query.empty + query.failed;
    }

    per_stream
        .into_iter()
        .filter(|(_, (count, misses))| *count >= REPEATED_QUERIES && misses * 2 > *count)
        .map(|(stream, (count, misses))| Gap {
            kind: GapKind::DataMap,
            rule: GapRule::RepeatedQueries,
            stream: Some(stream.to_string()),
            service: None,
            count,
            misses,
        })
        .collect()
}

/// The structure looked up again and again; the stream looked at most, when one was.
fn structure_lookups(lookups: usize, per_stream: &BTreeMap<String, usize>) -> Option<Gap> {
    (lookups >= STRUCTURE_LOOKUPS).then(|| Gap {
        kind: GapKind::DataMap,
        rule: GapRule::StructureLookups,
        stream: most(per_stream),
        service: None,
        count: lookups,
        misses: 0,
    })
}

/// Log or trace queries that named a service with data elsewhere and came back empty, while no
/// query of the same stream found that service.
fn service_mismatches(uses: &[ToolUse]) -> Vec<Gap> {
    let mut known: BTreeMap<String, String> = BTreeMap::new();

    for used in uses.iter().filter(|used| {
        matches!(
            used.role,
            Role::Read {
                signal: Signal::Metrics | Signal::Apm,
                ..
            }
        ) && used.outcome.is_data()
    }) {
        for service in &used.services {
            known
                .entry(service.to_lowercase())
                .or_insert_with(|| service.clone());
        }
    }

    let mut misses: BTreeMap<(String, String), usize> = BTreeMap::new();

    for used in uses {
        let Role::Read {
            signal: Signal::Logs | Signal::Traces,
            target,
        } = &used.role
        else {
            continue;
        };

        if used.outcome != Outcome::Empty {
            continue;
        }

        for (lower, service) in &known {
            if used.names_service(lower) && !found(uses, target, lower) {
                *misses.entry((target.clone(), service.clone())).or_default() += 1;
            }
        }
    }

    misses
        .into_iter()
        .map(|((stream, service), count)| Gap {
            kind: GapKind::FieldAlignment,
            rule: GapRule::ServiceMismatch,
            stream: Some(stream),
            service: Some(service),
            count,
            misses: count,
        })
        .collect()
}

/// Whether some query of `target` brought data back for the service.
fn found(uses: &[ToolUse], target: &str, service: &str) -> bool {
    uses.iter().any(|used| {
        matches!(&used.role, Role::Read { target: read, .. } if read == target)
            && used.outcome.is_data()
            && used.names_service(service)
    })
}

/// Raw logs or series pulled again and again without the tool that sums them up.
fn raw_pulls(tally: &Tally) -> Vec<Gap> {
    let mut gaps = Vec::new();
    let pulls = [
        (GapRule::RawLogPulls, &tally.log_pulls, LOG_PATTERNS_TOOL),
        (
            GapRule::RawMetricPulls,
            &tally.metric_pulls,
            METRIC_TREND_TOOL,
        ),
    ];

    for (rule, per_stream, tool) in pulls {
        let count = per_stream.values().sum::<usize>();

        if count >= RAW_PULLS && !tally.features.contains(tool) {
            gaps.push(Gap {
                kind: rule.kind(),
                rule,
                stream: most(per_stream),
                service: None,
                count,
                misses: 0,
            });
        }
    }

    gaps
}

/// The key counted most; the first in order on a tie.
fn most(counts: &BTreeMap<String, usize>) -> Option<String> {
    counts
        .iter()
        .fold(None::<(&String, usize)>, |best, (key, count)| match best {
            Some((_, top)) if top >= *count => best,
            _ => Some((key, *count)),
        })
        .map(|(key, _)| key.clone())
}
