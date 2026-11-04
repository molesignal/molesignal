// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What an application keeps of the events it is sent.
//!
//! A session is kept or dropped as a whole: its id is turned into a number from 0 to 99 (see
//! [`super::sample`]), and the session is kept when that is under the share it is kept with. The share is
//! the one of the first filter of the application that describes the session, or else the share of
//! the sessions no filter describes. A filter describes a session by facts that stay the same
//! while it lasts, so every event of a session, on any node, meets the same fate.
//!
//! The errors of a session that is dropped can be kept all the same. Such a row says it was kept
//! by [`ERRORS_FILTER`] and with a share of 100, as does every error when errors are kept, so
//! that errors are counted as they were.
//!
//! A session a test drove is kept unless a filter names the kind of session: a run has to be found
//! from the check that made it, and no figure of real users counts it anyway.

use serde_json::{Map, Value};

use super::sample::keeps;
use crate::domain::rum::{
    DEFAULT_FILTER, ERRORS_FILTER, MAX_SESSION_SAMPLE_RATE, RetentionCondition, RetentionFilter,
    RetentionOp, RumApplicationSettings,
};

/// What becomes of one event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The event is written, with the share it was kept with and the filter that kept it.
    Kept { rate: u32, filter: String },
    /// The session is not kept, and neither is this event.
    Dropped,
}

/// How sure a condition is to hold of a session of which only some facts are known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Truth {
    No,
    Maybe,
    Yes,
}

/// Whether every fact a condition may look at is known. A replay segment is sent with few of
/// them: what is not known may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Facts {
    Whole,
    Partial,
}

/// The retention an application is set to.
pub struct Retention<'a> {
    settings: &'a RumApplicationSettings,
}

impl<'a> Retention<'a> {
    pub fn of(settings: &'a RumApplicationSettings) -> Self {
        Self { settings }
    }

    /// Whether anything keeps a session of this bucket: false for most sessions of an application
    /// that keeps a small share, which saves what would be made of an event that is left out.
    /// `tested` says a test drove the session.
    pub fn may_keep(&self, session_id: &str, error: bool, tested: bool) -> bool {
        tested || (error && self.settings.keep_errors) || keeps(session_id, self.most())
    }

    /// What becomes of an event of a session, by what is known of the session: `row` is the event
    /// as it will be written, with the place and the browser the request came from.
    pub fn judge(&self, session_id: &str, row: &Map<String, Value>, error: bool) -> Verdict {
        let (rate, filter) = self.describing(row);
        let kept = keeps(session_id, rate);
        let errors = error && self.settings.keep_errors;

        match (kept, errors) {
            // Errors are all kept, so their share is the whole.
            (true, true) => Verdict::Kept {
                rate: MAX_SESSION_SAMPLE_RATE,
                filter,
            },
            (true, false) => Verdict::Kept { rate, filter },
            (false, true) => Verdict::Kept {
                rate: MAX_SESSION_SAMPLE_RATE,
                filter: ERRORS_FILTER.to_string(),
            },
            (false, false) => Verdict::Dropped,
        }
    }

    /// Whether a replay segment of a session is kept, going by the facts that came with it. When
    /// they leave open which filter describes the session, the segment is kept if any that might
    /// would keep the session.
    pub fn keeps_replay(&self, session_id: &str, known: &Map<String, Value>) -> bool {
        keeps(session_id, self.most_for(known))
    }

    /// The share and the id of the first filter that describes the row, or the share of the rest.
    fn describing(&self, row: &Map<String, Value>) -> (u32, String) {
        if self.keeps_whole(row) {
            return (MAX_SESSION_SAMPLE_RATE, DEFAULT_FILTER.to_string());
        }

        self.settings
            .retention_filters
            .iter()
            .find(|filter| holds(filter, row, Facts::Whole) == Truth::Yes)
            .map_or_else(
                || {
                    (
                        self.settings.session_sample_rate,
                        DEFAULT_FILTER.to_string(),
                    )
                },
                |filter| (filter.rate, filter.id.clone()),
            )
    }

    /// Whether a session is kept whatever the shares say: a test drove it, and no filter of the
    /// application looks at the kind of a session, so the shares are meant for real users.
    fn keeps_whole(&self, facts: &Map<String, Value>) -> bool {
        driven_by_a_test(facts) && !self.settings.retention_filters.iter().any(names_the_kind)
    }

    /// The most any share a session might be kept with: of the filters that might describe it, up
    /// to the first that certainly does, and of the rest when none certainly does.
    fn most_for(&self, known: &Map<String, Value>) -> u32 {
        if self.keeps_whole(known) {
            return MAX_SESSION_SAMPLE_RATE;
        }

        let mut most = 0;

        for filter in &self.settings.retention_filters {
            match holds(filter, known, Facts::Partial) {
                Truth::No => {}
                Truth::Maybe => most = most.max(filter.rate),
                Truth::Yes => return most.max(filter.rate),
            }
        }

        most.max(self.settings.session_sample_rate)
    }

    /// The most share any session of the application is kept with.
    fn most(&self) -> u32 {
        self.settings
            .retention_filters
            .iter()
            .map(|filter| filter.rate)
            .chain([self.settings.session_sample_rate])
            .max()
            .unwrap_or_default()
    }
}

/// Whether what is known of a session says a test drove it.
fn driven_by_a_test(facts: &Map<String, Value>) -> bool {
    facts
        .get("session.type")
        .and_then(Value::as_str)
        .is_some_and(|kind| !kind.trim().is_empty() && kind != "user")
}

/// Whether a filter looks at the kind of a session, and so the application says what becomes of
/// test runs.
fn names_the_kind(filter: &RetentionFilter) -> bool {
    filter
        .conditions
        .iter()
        .any(|condition| condition.field == "session.type")
}

/// Whether a filter describes a session: every condition of it holds.
fn holds(filter: &RetentionFilter, facts: &Map<String, Value>, known: Facts) -> Truth {
    filter
        .conditions
        .iter()
        .map(|condition| meets(condition, facts, known))
        .min()
        .unwrap_or(Truth::Yes)
}

fn meets(condition: &RetentionCondition, facts: &Map<String, Value>, known: Facts) -> Truth {
    let value = facts
        .get(&condition.field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        // A session that does not say what kind it is, is a person's.
        .or((known == Facts::Whole && condition.field == "session.type").then_some("user"));

    let Some(value) = value else {
        // No value is something no listed value is, unless the fact might be there and unknown.
        return match (known, condition.op) {
            (Facts::Partial, _) => Truth::Maybe,
            (Facts::Whole, RetentionOp::Is) => Truth::No,
            (Facts::Whole, RetentionOp::IsNot) => Truth::Yes,
        };
    };

    let listed = condition.values.iter().any(|item| same(item, value));

    match (condition.op, listed) {
        (RetentionOp::Is, true) | (RetentionOp::IsNot, false) => Truth::Yes,
        _ => Truth::No,
    }
}

/// Whether `text` is `pattern`, in which `*` stands for any text; case does not matter.
fn same(pattern: &str, text: &str) -> bool {
    let pattern = pattern.to_lowercase();
    let text = text.to_lowercase();
    let parts = pattern.split('*').collect::<Vec<_>>();

    let [first, middle @ .., last] = parts.as_slice() else {
        return pattern == text;
    };

    let Some(mut rest) = text.strip_prefix(first) else {
        return false;
    };

    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }

    rest.ends_with(last)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::rum::normalize_filters;

    fn row(fields: Value) -> Map<String, Value> {
        fields.as_object().unwrap().clone()
    }

    fn is(field: &str, values: &[&str]) -> RetentionCondition {
        RetentionCondition {
            field: field.into(),
            op: RetentionOp::Is,
            values: values.iter().map(|value| value.to_string()).collect(),
        }
    }

    fn is_not(field: &str, values: &[&str]) -> RetentionCondition {
        RetentionCondition {
            op: RetentionOp::IsNot,
            ..is(field, values)
        }
    }

    fn filter(name: &str, rate: u32, conditions: Vec<RetentionCondition>) -> RetentionFilter {
        RetentionFilter {
            id: String::new(),
            name: name.into(),
            rate,
            conditions,
        }
    }

    fn settings(
        rest: u32,
        filters: Vec<RetentionFilter>,
        keep_errors: bool,
    ) -> RumApplicationSettings {
        RumApplicationSettings {
            session_sample_rate: rest,
            retention_filters: normalize_filters(filters).unwrap(),
            keep_errors,
            ..Default::default()
        }
    }

    fn id(n: u32) -> String {
        format!("6f1c{n:08x}-1b7e-4d08-a5c3-90e2d41f7b68")
    }

    /// How many of 4000 sessions with these facts the settings keep, and the shares they say.
    fn count_kept(
        settings: &RumApplicationSettings,
        facts: &Value,
        error: bool,
    ) -> (usize, Vec<u32>) {
        let facts = row(facts.clone());
        let retention = Retention::of(settings);

        let mut shares = Vec::new();
        let mut count = 0;

        for n in 0..4_000 {
            if let Verdict::Kept { rate, .. } = retention.judge(&id(n), &facts, error) {
                count += 1;

                if !shares.contains(&rate) {
                    shares.push(rate);
                }
            }
        }

        (count, shares)
    }

    #[test]
    fn what_no_filter_describes_is_kept_by_the_share_left_for_the_rest() {
        let (kept, shares) = count_kept(
            &settings(25, Vec::new(), true),
            &json!({ "env": "prod" }),
            false,
        );

        assert_eq!(shares, [25]);
        assert!((kept as f64 - 1_000.0).abs() < 150.0, "{kept}");
    }

    #[test]
    fn the_first_filter_that_describes_a_session_decides_its_share() {
        let settings = settings(
            0,
            vec![
                filter("Beta", 100, vec![is("version", &["2.*-beta*"])]),
                filter("Production", 50, vec![is("env", &["prod"])]),
                filter("Everything", 10, Vec::new()),
            ],
            true,
        );

        // A session that every one of the shares keeps, so that what is said is the filter.
        let session = (0..).map(id).find(|id| keeps(id, 10)).unwrap();

        let by = |facts: Value| match Retention::of(&settings).judge(&session, &row(facts), false) {
            Verdict::Kept { rate, filter } => (rate, filter),
            Verdict::Dropped => (0, "dropped".into()),
        };

        assert_eq!(
            by(json!({ "version": "2.4.0-beta1", "env": "prod" })),
            (100, "beta".to_string())
        );
        assert_eq!(
            by(json!({ "version": "2.4.0", "env": "prod" })),
            (50, "production".to_string())
        );
        assert_eq!(
            by(json!({ "version": "2.4.0", "env": "dev" })),
            (10, "everything".to_string())
        );
    }

    #[test]
    fn a_share_is_kept_about_as_large_as_it_says_and_a_hundred_keeps_every_session() {
        let facts = json!({ "env": "prod", "geo.country_iso_code": "CN" });
        let settings = settings(
            100,
            vec![filter(
                "China",
                20,
                vec![is("geo.country_iso_code", &["cn"])],
            )],
            false,
        );

        let (count, shares) = count_kept(&settings, &facts, false);

        assert_eq!(shares, [20]);
        assert!((count as f64 - 800.0).abs() < 120.0, "{count}");

        let (count, shares) =
            count_kept(&settings, &json!({ "geo.country_iso_code": "JP" }), false);

        assert_eq!((count, shares), (4_000, vec![100]));
    }

    #[test]
    fn a_session_kept_at_a_lower_share_is_kept_at_a_higher_one() {
        let low = settings(10, Vec::new(), false);
        let high = settings(40, Vec::new(), false);
        let facts = row(json!({}));

        for n in 0..2_000 {
            let at_low = Retention::of(&low).judge(&id(n), &facts, false);

            if at_low != Verdict::Dropped {
                assert_ne!(
                    Retention::of(&high).judge(&id(n), &facts, false),
                    Verdict::Dropped
                );
            }
        }
    }

    #[test]
    fn is_not_holds_of_a_session_with_none_of_the_values_or_with_no_value() {
        let settings = settings(
            0,
            vec![filter(
                "Not dev",
                100,
                vec![is_not("env", &["dev", "staging"])],
            )],
            false,
        );

        let kept = |facts: Value| {
            matches!(
                Retention::of(&settings).judge(&id(7), &row(facts), false),
                Verdict::Kept { .. }
            )
        };

        assert!(kept(json!({ "env": "prod" })));
        assert!(kept(json!({})));
        assert!(!kept(json!({ "env": "DEV" })));
        assert!(!kept(json!({ "env": "staging" })));
    }

    #[test]
    fn a_session_that_says_no_kind_is_a_persons() {
        let settings = settings(
            0,
            vec![filter("People", 100, vec![is("session.type", &["user"])])],
            false,
        );

        let kept = |facts: Value| {
            matches!(
                Retention::of(&settings).judge(&id(7), &row(facts), false),
                Verdict::Kept { .. }
            )
        };

        assert!(kept(json!({})));
        assert!(kept(json!({ "session.type": "user" })));
        assert!(!kept(json!({ "session.type": "synthetics" })));
    }

    #[test]
    fn a_filter_with_conditions_needs_all_of_them_and_a_value_of_each() {
        let settings = settings(
            0,
            vec![filter(
                "Android in China",
                100,
                vec![
                    is("os.name", &["Android"]),
                    is("geo.country_iso_code", &["CN", "HK"]),
                ],
            )],
            false,
        );

        let kept = |facts: Value| {
            matches!(
                Retention::of(&settings).judge(&id(7), &row(facts), false),
                Verdict::Kept { .. }
            )
        };

        assert!(kept(
            json!({ "os.name": "android", "geo.country_iso_code": "hk" })
        ));
        assert!(!kept(
            json!({ "os.name": "Android", "geo.country_iso_code": "JP" })
        ));
        assert!(!kept(
            json!({ "os.name": "iOS", "geo.country_iso_code": "CN" })
        ));
        assert!(!kept(json!({ "geo.country_iso_code": "CN" })));
    }

    #[test]
    fn an_error_of_a_session_that_is_dropped_is_kept_with_the_whole_share() {
        let settings = settings(0, Vec::new(), true);
        let retention = Retention::of(&settings);
        let facts = row(json!({ "env": "prod" }));

        assert_eq!(retention.judge(&id(3), &facts, false), Verdict::Dropped);
        assert_eq!(
            retention.judge(&id(3), &facts, true),
            Verdict::Kept {
                rate: 100,
                filter: ERRORS_FILTER.into()
            }
        );
        assert!(retention.may_keep(&id(3), true, false));
        assert!(!retention.may_keep(&id(3), false, false));
    }

    #[test]
    fn with_errors_kept_every_error_has_the_whole_share_even_in_a_session_that_is_kept() {
        let settings = settings(100, vec![filter("Some", 30, Vec::new())], true);
        let retention = Retention::of(&settings);
        let facts = row(json!({}));

        let found = (0..200)
            .filter_map(|n| match retention.judge(&id(n), &facts, true) {
                Verdict::Kept { rate, filter } => Some((rate, filter)),
                Verdict::Dropped => None,
            })
            .collect::<Vec<_>>();

        // Every error was kept; those of sessions the filter keeps say which filter did, and the
        // others say they are errors. All of them are of the whole share.
        assert_eq!(found.len(), 200);
        assert!(found.iter().all(|(rate, _)| *rate == 100));
        assert!(found.iter().any(|(_, filter)| filter == "some"));
        assert!(found.iter().any(|(_, filter)| filter == ERRORS_FILTER));

        // A session that is kept says its own share for what is not an error.
        let kept = (0..200)
            .find_map(|n| match retention.judge(&id(n), &facts, false) {
                Verdict::Kept { rate, .. } => Some(rate),
                Verdict::Dropped => None,
            })
            .unwrap();

        assert_eq!(kept, 30);
    }

    #[test]
    fn without_errors_kept_an_error_is_one_of_its_session() {
        let settings = settings(0, Vec::new(), false);
        let retention = Retention::of(&settings);

        assert_eq!(
            retention.judge(&id(3), &row(json!({})), true),
            Verdict::Dropped
        );
        assert!(!retention.may_keep(&id(3), true, false));
    }

    #[test]
    fn a_session_is_only_considered_when_something_could_keep_it() {
        let settings = settings(10, vec![filter("A", 60, vec![is("env", &["prod"])])], false);
        let retention = Retention::of(&settings);

        let few = (0..1_000)
            .filter(|n| retention.may_keep(&id(*n), false, false))
            .count();

        // The most share any filter has is 60.
        assert!((few as f64 - 600.0).abs() < 90.0, "{few}");

        // What may be kept is never less than what is.
        let facts = row(json!({ "env": "prod" }));

        for n in 0..1_000 {
            if retention.judge(&id(n), &facts, false) != Verdict::Dropped {
                assert!(retention.may_keep(&id(n), false, false));
            }
        }
    }

    #[test]
    fn a_replay_is_kept_when_a_filter_that_might_describe_its_session_would_keep_it() {
        let settings = settings(
            5,
            vec![
                filter("Prod", 100, vec![is("env", &["prod"])]),
                filter("Chrome", 40, vec![is("browser.name", &["Chrome"])]),
            ],
            false,
        );
        let retention = Retention::of(&settings);

        let kept = |facts: Value| {
            (0..1_000)
                .filter(|n| retention.keeps_replay(&id(*n), &row(facts.clone())))
                .count()
        };

        // The environment is not known: any session might be a production one.
        assert_eq!(kept(json!({ "browser.name": "Firefox" })), 1_000);
        // It is, and it is not production: only the other filter, or the rest, might keep it.
        let not_prod = kept(json!({ "env": "dev", "browser.name": "Chrome" }));

        assert!((not_prod as f64 - 400.0).abs() < 80.0, "{not_prod}");

        let rest = kept(json!({ "env": "dev", "browser.name": "Firefox" }));

        assert!((rest as f64 - 50.0).abs() < 40.0, "{rest}");

        // It is production: the filter that certainly describes it decides, and no later one.
        assert_eq!(kept(json!({ "env": "prod" })), 1_000);
    }

    #[test]
    fn wildcards_stand_for_any_text_and_case_does_not_matter() {
        assert!(same("prod", "PROD"));
        assert!(same("2.*", "2.4.1"));
        assert!(!same("2.*", "12.4.1"));
        assert!(same("*-beta", "2.4.0-beta"));
        assert!(!same("*-beta", "2.4.0-beta2"));
        assert!(same("*beta*", "2.4.0-beta2"));
        assert!(same("a*b*c", "aXXbYYc"));
        assert!(same("a*b*c", "abc"));
        assert!(!same("a*b*c", "acb"));
        assert!(same("*", ""));
        assert!(same("*", "anything"));
        assert!(!same("ab*ba", "aba"));
        assert!(same("ab*ba", "abba"));
        assert!(!same("prod", "production"));
    }
}
