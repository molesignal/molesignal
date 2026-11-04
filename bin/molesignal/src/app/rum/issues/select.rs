// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Which of the issues seen in a range a list shows, and in which order.
//!
//! The rows have two sources: the numbers of the range come from the errors themselves, the state
//! and the history from the issues kept. This joins them, so it stays free of both.

use std::collections::HashMap;

use serde::Serialize;

use crate::{
    domain::rum::{RumIssue, RumIssueStatus},
    shared::{Error, Result, ids::Id},
};

/// What the errors of one issue come to in the range.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Numbers {
    pub sessions: u64,
    pub users: u64,
    pub occurrences: u64,
}

/// One row of the list: the issue, and its numbers.
#[derive(Debug, Serialize)]
pub struct IssueView {
    #[serde(flatten)]
    pub issue: RumIssue,
    #[serde(flatten)]
    pub numbers: Numbers,
    /// Occurrences in each of the equal parts the range is cut into, oldest first.
    pub trend: Vec<u64>,
}

/// The states a list shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusFilter {
    /// Not resolved and not set aside: open, investigating and regressed.
    Unresolved,
    Only(RumIssueStatus),
    All,
}

impl StatusFilter {
    pub fn parse(text: Option<&str>) -> Result<Self> {
        match text.map(str::trim).filter(|text| !text.is_empty()) {
            None | Some("unresolved") => Ok(Self::Unresolved),
            Some("all") => Ok(Self::All),
            Some(text) => RumIssueStatus::parse(text).map(Self::Only).ok_or_else(|| {
                Error::invalid(
                    "status must be unresolved, all, open, investigating, regressed, resolved or ignored",
                )
            }),
        }
    }

    fn admits(self, status: RumIssueStatus) -> bool {
        match self {
            Self::Unresolved => status.is_unresolved(),
            Self::Only(only) => only == status,
            Self::All => true,
        }
    }
}

/// Who has an issue, as a filter: somebody, or nobody.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder {
    Anyone,
    Nobody,
    Is(Id),
}

impl Holder {
    pub fn parse(text: Option<&str>) -> Self {
        match text.map(str::trim).filter(|text| !text.is_empty()) {
            None => Self::Anyone,
            Some("none") => Self::Nobody,
            Some(id) => Self::Is(Id::from_string(id)),
        }
    }

    fn admits(&self, holder: Option<&Id>) -> bool {
        match (self, holder) {
            (Self::Anyone, _) => true,
            (Self::Nobody, holder) => holder.is_none(),
            (Self::Is(wanted), Some(holder)) => wanted == holder,
            (Self::Is(_), None) => false,
        }
    }
}

/// How the rows are ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Sessions,
    Users,
    Occurrences,
    LastSeen,
    FirstSeen,
    Status,
}

impl Sort {
    pub fn parse(text: Option<&str>) -> Result<Self> {
        Ok(match text.map(str::trim).filter(|text| !text.is_empty()) {
            None | Some("sessions") => Self::Sessions,
            Some("users") => Self::Users,
            Some("occurrences") => Self::Occurrences,
            Some("last_seen") => Self::LastSeen,
            Some("first_seen") => Self::FirstSeen,
            Some("status") => Self::Status,
            Some(_) => {
                return Err(Error::invalid(
                    "sort must be sessions, users, occurrences, last_seen, first_seen or status",
                ));
            }
        })
    }

    /// The measure the issues are ranked by when the errors are counted, so that the ones that
    /// matter are among those that are read.
    pub fn measure(self) -> &'static str {
        match self {
            Self::Users => "users",
            Self::Occurrences => "occurrences",
            _ => "sessions",
        }
    }

    /// The order a column reads in when none is asked for: the biggest or the latest first.
    pub fn default_descending(self) -> bool {
        !matches!(self, Self::Status)
    }
}

/// What a list asks of the issues kept.
#[derive(Debug)]
pub struct Wanted {
    pub status: StatusFilter,
    pub assignee: Holder,
    pub team: Holder,
    pub search: String,
    /// Only issues that are new in the range or went back in it.
    pub fresh_since: Option<i64>,
    pub sort: Sort,
    pub descending: bool,
}

/// Rank of a state in a list by state: what needs somebody first.
fn rank(status: RumIssueStatus) -> u8 {
    match status {
        RumIssueStatus::Regressed => 0,
        RumIssueStatus::Open => 1,
        RumIssueStatus::Investigating => 2,
        RumIssueStatus::Resolved => 3,
        RumIssueStatus::Ignored => 4,
    }
}

fn matches_search(issue: &RumIssue, search: &str) -> bool {
    if search.is_empty() {
        return true;
    }

    [&issue.error_type, &issue.message, &issue.location]
        .into_iter()
        .any(|text| text.to_lowercase().contains(search))
}

/// The issues of `seen` that the list shows, in order. An issue that is seen but not kept (the
/// tracker has not written it yet) is not listed.
pub fn select(
    seen: Vec<(String, Numbers)>,
    kept: Vec<RumIssue>,
    wanted: &Wanted,
) -> Vec<IssueView> {
    let mut kept = kept
        .into_iter()
        .map(|issue| (issue.id.clone(), issue))
        .collect::<HashMap<_, _>>();

    let search = wanted.search.trim().to_lowercase();

    let mut rows = seen
        .into_iter()
        .filter_map(|(id, numbers)| kept.remove(&id).map(|issue| (issue, numbers)))
        .filter(|(issue, _)| wanted.status.admits(issue.status))
        .filter(|(issue, _)| wanted.assignee.admits(issue.assignee_id.as_ref()))
        .filter(|(issue, _)| wanted.team.admits(issue.team_id.as_ref()))
        .filter(|(issue, _)| matches_search(issue, &search))
        .filter(|(issue, _)| {
            wanted.fresh_since.is_none_or(|since| {
                issue.first_seen_at.0 >= since
                    || (issue.status == RumIssueStatus::Regressed
                        && issue.regressed_at.is_some_and(|at| at.0 >= since))
            })
        })
        .map(|(issue, numbers)| IssueView {
            issue,
            numbers,
            trend: Vec::new(),
        })
        .collect::<Vec<_>>();

    rows.sort_by(|left, right| {
        let order = match wanted.sort {
            Sort::Sessions => left.numbers.sessions.cmp(&right.numbers.sessions),
            Sort::Users => left.numbers.users.cmp(&right.numbers.users),
            Sort::Occurrences => left.numbers.occurrences.cmp(&right.numbers.occurrences),
            Sort::LastSeen => left.issue.last_seen_at.cmp(&right.issue.last_seen_at),
            Sort::FirstSeen => left.issue.first_seen_at.cmp(&right.issue.first_seen_at),
            Sort::Status => rank(left.issue.status).cmp(&rank(right.issue.status)),
        };

        let order = if wanted.descending {
            order.reverse()
        } else {
            order
        };

        // Rows that tie come in the order of the most sessions, then of their ids, so that a list
        // is the same each time it is read.
        order
            .then_with(|| right.numbers.sessions.cmp(&left.numbers.sessions))
            .then_with(|| left.issue.id.cmp(&right.issue.id))
    });

    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::time::TimestampMicros;

    fn issue(id: &str, status: RumIssueStatus, first: i64) -> RumIssue {
        RumIssue {
            status,
            error_type: "TypeError".into(),
            message: format!("message of {id}"),
            location: "src/cart.ts:8".into(),
            ..RumIssue::opened(id.into(), "web".into(), TimestampMicros(first), None)
        }
    }

    fn numbers(sessions: u64) -> Numbers {
        Numbers {
            sessions,
            users: sessions / 2,
            occurrences: sessions * 3,
        }
    }

    fn wanted() -> Wanted {
        Wanted {
            status: StatusFilter::Unresolved,
            assignee: Holder::Anyone,
            team: Holder::Anyone,
            search: String::new(),
            fresh_since: None,
            sort: Sort::Sessions,
            descending: true,
        }
    }

    fn ids(rows: &[IssueView]) -> Vec<&str> {
        rows.iter().map(|row| row.issue.id.as_str()).collect()
    }

    fn fixture() -> (Vec<(String, Numbers)>, Vec<RumIssue>) {
        (
            vec![
                ("a".into(), numbers(10)),
                ("b".into(), numbers(50)),
                ("c".into(), numbers(30)),
                ("d".into(), numbers(40)),
                ("gone".into(), numbers(99)),
            ],
            vec![
                issue("a", RumIssueStatus::Open, 100),
                issue("b", RumIssueStatus::Regressed, 10),
                issue("c", RumIssueStatus::Resolved, 300),
                issue("d", RumIssueStatus::Ignored, 400),
            ],
        )
    }

    #[test]
    fn what_is_unresolved_is_listed_by_default_the_biggest_first() {
        let (seen, kept) = fixture();

        let rows = select(seen, kept, &wanted());

        assert_eq!(ids(&rows), ["b", "a"]);
        assert_eq!(rows[0].numbers.sessions, 50);
    }

    #[test]
    fn a_state_a_person_a_team_and_words_narrow_the_list() {
        let (seen, mut kept) = fixture();

        kept[0].assignee_id = Some(Id::from_string("ada"));
        kept[0].team_id = Some(Id::from_string("web"));

        let all = Wanted {
            status: StatusFilter::All,
            ..wanted()
        };

        assert_eq!(ids(&select(seen.clone(), kept.clone(), &all)).len(), 4);

        let resolved = Wanted {
            status: StatusFilter::Only(RumIssueStatus::Resolved),
            ..wanted()
        };

        assert_eq!(ids(&select(seen.clone(), kept.clone(), &resolved)), ["c"]);

        let mine = Wanted {
            status: StatusFilter::All,
            assignee: Holder::Is(Id::from_string("ada")),
            ..wanted()
        };

        assert_eq!(ids(&select(seen.clone(), kept.clone(), &mine)), ["a"]);

        let nobody = Wanted {
            status: StatusFilter::All,
            assignee: Holder::Nobody,
            team: Holder::Nobody,
            ..wanted()
        };

        assert_eq!(
            ids(&select(seen.clone(), kept.clone(), &nobody)),
            ["b", "d", "c"]
        );

        let words = Wanted {
            status: StatusFilter::All,
            search: " MESSAGE of c ".into(),
            ..wanted()
        };

        assert_eq!(ids(&select(seen, kept, &words)), ["c"]);
    }

    #[test]
    fn only_what_is_new_or_went_back_in_the_range_is_fresh() {
        let (seen, mut kept) = fixture();

        kept[1].regressed_at = Some(TimestampMicros(250));

        let fresh = Wanted {
            status: StatusFilter::All,
            fresh_since: Some(200),
            ..wanted()
        };

        // `a` was first seen before the range, `b` went back in it, `c` and `d` are new.
        assert_eq!(ids(&select(seen, kept, &fresh)), ["b", "d", "c"]);
    }

    #[test]
    fn the_order_is_the_one_asked_for_and_ties_are_settled() {
        let (seen, kept) = fixture();

        let by_first_seen = Wanted {
            status: StatusFilter::All,
            sort: Sort::FirstSeen,
            descending: false,
            ..wanted()
        };

        assert_eq!(
            ids(&select(seen.clone(), kept.clone(), &by_first_seen)),
            ["b", "a", "c", "d"]
        );

        let by_status = Wanted {
            status: StatusFilter::All,
            sort: Sort::Status,
            descending: false,
            ..wanted()
        };

        assert_eq!(ids(&select(seen, kept, &by_status)), ["b", "a", "c", "d"]);

        let tied = vec![("x".into(), numbers(5)), ("y".into(), numbers(5))];
        let kept = vec![
            issue("x", RumIssueStatus::Open, 1),
            issue("y", RumIssueStatus::Open, 1),
        ];

        assert_eq!(ids(&select(tied, kept, &wanted())), ["x", "y"]);
    }

    #[test]
    fn what_is_asked_for_is_checked() {
        assert_eq!(StatusFilter::parse(None).unwrap(), StatusFilter::Unresolved);
        assert_eq!(StatusFilter::parse(Some("all")).unwrap(), StatusFilter::All);
        assert_eq!(
            StatusFilter::parse(Some("regressed")).unwrap(),
            StatusFilter::Only(RumIssueStatus::Regressed)
        );
        assert!(StatusFilter::parse(Some("closed")).is_err());
        assert_eq!(Sort::parse(Some("users")).unwrap().measure(), "users");
        assert_eq!(
            Sort::parse(Some("first_seen")).unwrap().measure(),
            "sessions"
        );
        assert!(Sort::parse(Some("name")).is_err());
        assert_eq!(Holder::parse(Some("none")), Holder::Nobody);
        assert_eq!(Holder::parse(Some("  ")), Holder::Anyone);
    }
}
