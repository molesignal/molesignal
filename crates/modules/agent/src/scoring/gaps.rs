// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The data gaps of a period: what the trajectories of its answers found, counted by kind and
//! grouped by rule, stream and service.
//!
//! Personal chats count in the totals only: what a personal chat queried is not listed, the same
//! way its question and answer are not.

use std::collections::BTreeMap;

use kernel::{ids::Id, time::TimestampMicros};
use serde::Serialize;

use super::{
    TargetSource,
    trajectory::{GapKind, GapRule, Trajectory},
};

/// At most this many groups are listed, the most common first.
pub const MAX_GROUPS: usize = 50;
/// At most this many answers are given for a group, the newest first.
pub const EXAMPLES_PER_GROUP: usize = 5;

/// An answer's trajectory as kept with its scored subject.
#[derive(Debug, Clone, PartialEq)]
pub struct TrajectoryRow {
    pub source: TargetSource,
    pub target_id: String,
    pub chat_id: Option<Id>,
    /// The beginning of the question; empty for personal chats.
    pub question: String,
    pub target_at: TimestampMicros,
    pub trajectory: Trajectory,
}

/// The gaps of a period.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataGaps {
    /// Answers that called tools.
    pub answers: usize,
    /// Of those, the answers with at least one gap.
    pub with_gaps: usize,
    pub kinds: Vec<KindCount>,
    pub groups: Vec<GapGroup>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KindCount {
    pub kind: GapKind,
    pub answers: usize,
}

/// One gap found again and again: the same rule on the same stream (and service).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GapGroup {
    pub kind: GapKind,
    pub rule: GapRule,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    /// Answers that found it.
    pub answers: usize,
    /// The calls the rule counted, over those answers.
    pub calls: usize,
    pub misses: usize,
    pub last_at: TimestampMicros,
    pub examples: Vec<GapExample>,
}

/// An answer that found a gap: where to read it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GapExample {
    pub target_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<Id>,
    pub source: TargetSource,
    pub question: String,
    pub target_at: TimestampMicros,
}

type GroupKey = (GapRule, Option<String>, Option<String>);

/// Counts and groups the gaps of `rows`, which come newest first.
pub fn aggregate(rows: &[TrajectoryRow]) -> DataGaps {
    let mut kinds: BTreeMap<GapKind, usize> = BTreeMap::new();
    let mut groups: BTreeMap<GroupKey, GapGroup> = BTreeMap::new();
    let mut with_gaps = 0;

    for row in rows {
        let trajectory = &row.trajectory;

        if !trajectory.gaps.is_empty() {
            with_gaps += 1;
        }

        for kind in GapKind::ALL {
            if trajectory.has(kind) {
                *kinds.entry(kind).or_default() += 1;
            }
        }

        if row.source.is_private() {
            continue;
        }

        // An answer that finds the same gap twice counts once for it, with both counts.
        let mut found: BTreeMap<GroupKey, (GapKind, usize, usize)> = BTreeMap::new();

        for gap in &trajectory.gaps {
            let entry = found
                .entry((gap.rule, gap.stream.clone(), gap.service.clone()))
                .or_insert((gap.kind, 0, 0));

            entry.1 += gap.count;
            entry.2 += gap.misses;
        }

        for (key, (kind, calls, misses)) in found {
            let (rule, stream, service) = key.clone();

            let group = groups.entry(key).or_insert_with(|| GapGroup {
                kind,
                rule,
                stream,
                service,
                answers: 0,
                calls: 0,
                misses: 0,
                last_at: row.target_at,
                examples: Vec::new(),
            });

            group.answers += 1;
            group.calls += calls;
            group.misses += misses;
            group.last_at = TimestampMicros(group.last_at.0.max(row.target_at.0));

            if group.examples.len() < EXAMPLES_PER_GROUP {
                group.examples.push(GapExample {
                    target_id: row.target_id.clone(),
                    chat_id: row.chat_id.clone(),
                    source: row.source,
                    question: row.question.clone(),
                    target_at: row.target_at,
                });
            }
        }
    }

    let mut groups = groups.into_values().collect::<Vec<_>>();

    groups.sort_by(|left, right| {
        right
            .answers
            .cmp(&left.answers)
            .then(right.calls.cmp(&left.calls))
            .then(right.last_at.0.cmp(&left.last_at.0))
    });
    groups.truncate(MAX_GROUPS);

    DataGaps {
        answers: rows.len(),
        with_gaps,
        kinds: GapKind::ALL
            .into_iter()
            .map(|kind| KindCount {
                kind,
                answers: kinds.get(&kind).copied().unwrap_or(0),
            })
            .collect(),
        groups,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scoring::trajectory::Gap;

    fn gap(rule: GapRule, stream: &str, service: Option<&str>, count: usize) -> Gap {
        Gap {
            kind: rule.kind(),
            rule,
            stream: Some(stream.into()),
            service: service.map(str::to_string),
            count,
            misses: count / 2,
        }
    }

    fn row(id: &str, source: TargetSource, at: i64, gaps: Vec<Gap>) -> TrajectoryRow {
        TrajectoryRow {
            source,
            target_id: id.into(),
            chat_id: Some(Id::from_string(format!("chat-{id}"))),
            question: format!("question {id}"),
            target_at: TimestampMicros(at),
            trajectory: Trajectory {
                calls: 4,
                gaps,
                ..Trajectory::default()
            },
        }
    }

    #[test]
    fn gaps_are_counted_by_kind_and_grouped_by_rule_and_stream() {
        let rows = vec![
            row(
                "m3",
                TargetSource::TeamChat,
                30,
                vec![gap(GapRule::RepeatedQueries, "nginx", None, 4)],
            ),
            row(
                "m2",
                TargetSource::Automation,
                20,
                vec![
                    gap(GapRule::RepeatedQueries, "nginx", None, 3),
                    gap(GapRule::ServiceMismatch, "app_logs", Some("order-api"), 1),
                ],
            ),
            row("m1", TargetSource::TeamChat, 10, Vec::new()),
        ];

        let gaps = aggregate(&rows);

        assert_eq!(gaps.answers, 3);
        assert_eq!(gaps.with_gaps, 2);
        assert_eq!(
            gaps.kinds,
            vec![
                KindCount {
                    kind: GapKind::DataMap,
                    answers: 2
                },
                KindCount {
                    kind: GapKind::FieldAlignment,
                    answers: 1
                },
                KindCount {
                    kind: GapKind::FeatureTools,
                    answers: 0
                },
            ]
        );

        let first = &gaps.groups[0];
        assert_eq!(first.rule, GapRule::RepeatedQueries);
        assert_eq!(first.stream.as_deref(), Some("nginx"));
        assert_eq!((first.answers, first.calls), (2, 7));
        assert_eq!(first.last_at, TimestampMicros(30));
        assert_eq!(
            first
                .examples
                .iter()
                .map(|example| example.target_id.as_str())
                .collect::<Vec<_>>(),
            vec!["m3", "m2"]
        );

        assert_eq!(gaps.groups[1].service.as_deref(), Some("order-api"));
    }

    #[test]
    fn personal_chats_count_in_the_totals_but_are_never_listed() {
        let rows = vec![row(
            "private",
            TargetSource::PersonalChat,
            5,
            vec![gap(GapRule::RawLogPulls, "secret_stream", None, 2)],
        )];

        let gaps = aggregate(&rows);

        assert_eq!(gaps.with_gaps, 1);
        assert_eq!(gaps.kinds[2].answers, 1);
        assert!(gaps.groups.is_empty());
    }

    #[test]
    fn an_answer_counts_once_per_group_and_examples_are_capped() {
        let mut rows = vec![row(
            "twice",
            TargetSource::TeamChat,
            100,
            vec![
                gap(GapRule::ServiceMismatch, "logs", Some("a"), 1),
                gap(GapRule::ServiceMismatch, "logs", Some("a"), 1),
            ],
        )];

        for index in 0..8 {
            rows.push(row(
                &format!("m{index}"),
                TargetSource::TeamChat,
                50 - index,
                vec![gap(GapRule::ServiceMismatch, "logs", Some("a"), 1)],
            ));
        }

        let gaps = aggregate(&rows);
        let group = &gaps.groups[0];

        assert_eq!(group.answers, 9);
        assert_eq!(group.calls, 10);
        assert_eq!(group.examples.len(), EXAMPLES_PER_GROUP);
        assert_eq!(group.examples[0].target_id, "twice");
    }
}
