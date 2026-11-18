// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/insight`: reviews Mole Agent usage over a period of time.
//!
//! The scope follows the session: team sessions cover that team's sessions and automations;
//! personal sessions cover the user's personal sessions, sessions of teams they belong to, and
//! automations of the user and those teams. Both session types include organization-scoped
//! automations. Read-only; changes nothing.

use std::collections::BTreeMap;

use kernel::{ids::Id, time::TimestampMicros};
use serde::Serialize;

/// The maximum number of days to look back.
pub const MAX_DAYS: i64 = 90;
pub const DEFAULT_DAYS: i64 = 30;

/// Whose data to review: one user's personal sessions in the organization (empty for team
/// sessions), plus these teams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsightScope {
    pub org_id: Id,
    pub owner_id: Option<Id>,
    pub team_ids: Vec<Id>,
}

/// The number of times a name is used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Count {
    pub name: String,
    pub count: i64,
}

/// The number of calls and errors for a tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolUse {
    pub name: String,
    pub calls: i64,
    pub errors: i64,
}

/// Runs of an automation, counted by status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AutomationUse {
    pub name: String,
    pub runs: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Insights {
    pub since: TimestampMicros,
    /// The number of sessions with user questions and the number of questions, excluding
    /// automations.
    pub chats: i64,
    pub questions: i64,
    /// Answers (Run), counted by status.
    pub runs: BTreeMap<String, i64>,
    pub helpful: i64,
    pub unhelpful: i64,
    /// Reasons for `Not helpful`.
    pub unhelpful_reasons: Vec<Count>,
    /// The most frequently called tools, up to 20.
    pub tools: Vec<ToolUse>,
    /// Skills used: those invoked by commands, plus those read by the model itself.
    pub skills: Vec<Count>,
    pub automations: Vec<AutomationUse>,
}

/// Merges a set of (name, count) pairs, sorting by count descending and then by name for ties.
pub fn ranked(counts: impl IntoIterator<Item = (String, i64)>) -> Vec<Count> {
    let mut merged = BTreeMap::<String, i64>::new();

    for (name, count) in counts {
        if !name.trim().is_empty() {
            *merged.entry(name).or_default() += count;
        }
    }

    let mut ranked = merged
        .into_iter()
        .map(|(name, count)| Count { name, count })
        .collect::<Vec<_>>();

    ranked.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_merged_and_ranked() {
        let ranked = ranked([
            ("incident-triage".to_string(), 2),
            ("init".to_string(), 3),
            ("incident-triage".to_string(), 2),
            (" ".to_string(), 9),
        ]);

        assert_eq!(
            ranked,
            vec![
                Count {
                    name: "incident-triage".into(),
                    count: 4
                },
                Count {
                    name: "init".into(),
                    count: 3
                },
            ]
        );
    }
}
