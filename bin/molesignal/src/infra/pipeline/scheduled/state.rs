// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Where each source of a scheduled pipeline has been read up to, and the windows being retried.
//!
//! A run reads every source from where the last successful read of it ended up to the run's time,
//! less the pipeline's delay for late events. One window starts where the last one ended, so an
//! event is read once if it arrives within the delay. A source that has never been read starts the
//! lookback before. A run reads at most [`Timing::span`] of a source; a source further behind is
//! due again at once, so a pipeline catches up a stretch at a time.
//!
//! A window whose read or write failed keeps its place: it is tried again after a backoff, and the
//! destinations that already took it are left out then. When the retries run out, or when the
//! failure is the pipeline's own configuration, the window is given up or kept as described on
//! [`RunState::after`].

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::pipeline::scheduled::RetryPolicy;

const MICROS_PER_SEC: i64 = 1_000_000;
/// The least a run reads of a source that is behind, however short the interval.
const MIN_SPAN_MICROS: i64 = 3_600 * MICROS_PER_SEC;
/// The first retry waits this long; every further one twice as long, up to the run interval.
const FIRST_BACKOFF_MICROS: i64 = 30 * MICROS_PER_SEC;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunState {
    #[serde(default)]
    pub sources: BTreeMap<String, SourceState>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceState {
    /// Events of the source from before this time (microseconds) have been handled.
    pub read_until: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<Retry>,
    /// Why the source is kept where it is: its last window failed for a reason that trying again
    /// would meet again (the configuration). It waits for the regular run instead of catching up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<String>,
}

/// A window that failed and is waiting to be tried again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retry {
    /// Attempts at the window that failed so far.
    pub failures: u32,
    /// When the next attempt is due (microseconds).
    pub next_at: i64,
    /// Where the window ends.
    pub until: i64,
    /// Destinations that already took the window.
    #[serde(default)]
    pub done: BTreeSet<String>,
    /// What the last attempt failed on.
    pub error: String,
}

/// A stretch of one source a run reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub source: String,
    pub start: i64,
    pub end: i64,
    /// Destinations that already took the window in an earlier attempt.
    pub done: BTreeSet<String>,
    /// 1 for the first attempt at the window.
    pub attempt: u32,
}

/// What became of a source's window in a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceResult {
    /// Read and taken by every destination.
    Done,
    /// The read, or a write to a destination, failed. `done` holds the destinations that took the
    /// window anyway. A `permanent` failure is the pipeline's configuration: trying the same again
    /// would fail the same way.
    Failed {
        done: BTreeSet<String>,
        error: String,
        permanent: bool,
    },
}

/// When a pipeline runs, and what it reads at most.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    pub every: i64,
    pub lookback: i64,
    pub delay: i64,
}

impl Timing {
    pub fn from_secs(every: i64, lookback: i64, delay: i64) -> Self {
        Self {
            every: every.max(1) * MICROS_PER_SEC,
            lookback: lookback.max(1) * MICROS_PER_SEC,
            delay: delay.max(0) * MICROS_PER_SEC,
        }
    }

    /// The most a run reads of one source.
    pub fn span(&self) -> i64 {
        self.every.max(self.lookback).max(MIN_SPAN_MICROS)
    }

    /// How long a window waits after its `failures`-th failure: 30 s, doubling, up to the interval.
    pub fn backoff(&self, failures: u32) -> i64 {
        let doubled = FIRST_BACKOFF_MICROS.saturating_mul(1 << failures.saturating_sub(1).min(20));

        doubled.min(self.every.max(FIRST_BACKOFF_MICROS))
    }
}

impl RunState {
    /// The state saved with a pipeline. One that cannot be read is no state: every source starts
    /// over, which reads the lookback again rather than nothing.
    pub fn from_value(value: &Value) -> Self {
        serde_json::from_value(value.clone()).unwrap_or_default()
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    /// The windows a run at `now` reads: a retry whose time has come, or the stretch since the
    /// last read. A source with nothing new, or waiting for its retry, has none.
    pub fn windows(&self, sources: &[String], timing: &Timing, now: i64) -> Vec<Window> {
        let horizon = now - timing.delay;

        sources
            .iter()
            .filter_map(|source| {
                let state = self.sources.get(source);

                if let Some(state) = state
                    && let Some(retry) = &state.retry
                {
                    return (now >= retry.next_at).then(|| Window {
                        source: source.clone(),
                        start: state.read_until,
                        end: retry.until,
                        done: retry.done.clone(),
                        attempt: retry.failures + 1,
                    });
                }

                let start = state.map_or(horizon - timing.lookback, |state| state.read_until);
                let end = horizon.min(start + timing.span());

                (end > start).then(|| Window {
                    source: source.clone(),
                    start,
                    end,
                    done: BTreeSet::new(),
                    attempt: 1,
                })
            })
            .collect()
    }

    /// Whether a run is due before the regular interval: a retry's time has come, or a source is
    /// further behind than one run reads.
    pub fn due_early(&self, sources: &[String], timing: &Timing, now: i64) -> bool {
        let horizon = now - timing.delay;

        sources.iter().any(|source| match self.sources.get(source) {
            Some(SourceState {
                retry: Some(retry), ..
            }) => now >= retry.next_at,
            // Held by its configuration: catching up at once would fail the same way at every tick.
            Some(state) => state.held.is_none() && state.read_until + timing.span() < horizon,
            None => false,
        })
    }

    /// The state after a run read `windows` and they came to `results`, and what the run gave up.
    ///
    /// - A window that was done moves its source on to the window's end.
    /// - A window that failed for a reason that would come again (the configuration) keeps its
    ///   place, held, and no retry is planned: the next regular run reads from there again, so
    ///   nothing is lost while the pipeline is being fixed.
    /// - A window that failed otherwise is tried again after a backoff, until `policy` has no
    ///   retries left. It is then given up: the source moves on, and the note says what was skipped.
    ///
    /// Sources no longer read are forgotten; sources the run did not read are kept as they were.
    pub fn after(
        &self,
        sources: &[String],
        windows: &[Window],
        results: &BTreeMap<String, SourceResult>,
        timing: &Timing,
        policy: RetryPolicy,
        now: i64,
    ) -> (RunState, Vec<String>) {
        let mut next = RunState::default();
        let mut skipped = Vec::new();

        for source in sources {
            let previous = self.sources.get(source);
            let window = windows.iter().find(|window| &window.source == source);

            let state = match (window, results.get(source)) {
                (Some(window), Some(SourceResult::Done)) => SourceState {
                    read_until: window.end,
                    ..SourceState::default()
                },
                (
                    Some(window),
                    Some(SourceResult::Failed {
                        permanent: true,
                        error,
                        ..
                    }),
                ) => SourceState {
                    read_until: window.start,
                    retry: None,
                    held: Some(error.clone()),
                },
                (Some(window), Some(SourceResult::Failed { done, error, .. })) => {
                    if window.attempt > policy.run_retries {
                        skipped.push(format!(
                            "gave up on `{source}` after {} attempt(s); its events from {} to {} \
                             were not processed: {error}",
                            window.attempt,
                            micros_text(window.start),
                            micros_text(window.end),
                        ));

                        SourceState {
                            read_until: window.end,
                            ..SourceState::default()
                        }
                    } else {
                        SourceState {
                            read_until: window.start,
                            retry: Some(Retry {
                                failures: window.attempt,
                                next_at: now + timing.backoff(window.attempt),
                                until: window.end,
                                done: window.done.union(done).cloned().collect(),
                                error: error.clone(),
                            }),
                            held: None,
                        }
                    }
                }
                _ => match previous {
                    Some(previous) => previous.clone(),
                    None => continue,
                },
            };

            next.sources.insert(source.clone(), state);
        }

        (next, skipped)
    }

    /// The retry the pipeline is waiting for soonest, as (failures so far, when it is due).
    pub fn pending_retry(&self) -> Option<(u32, i64)> {
        self.sources
            .values()
            .filter_map(|state| state.retry.as_ref())
            .map(|retry| (retry.failures, retry.next_at))
            .min_by_key(|(_, next_at)| *next_at)
    }
}

fn micros_text(micros: i64) -> String {
    chrono::DateTime::from_timestamp_micros(micros)
        .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_else(|| micros.to_string())
}

#[cfg(test)]
mod tests;
