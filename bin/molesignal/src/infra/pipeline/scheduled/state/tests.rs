// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::*;

const S: i64 = MICROS_PER_SEC;
const NOW: i64 = 1_800_000_000 * S;

/// Every 5 minutes, a first run reaching 10 minutes back, a minute's wait for late events.
fn timing() -> Timing {
    Timing::from_secs(300, 600, 60)
}

fn sources(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn failed(done: &[&str], permanent: bool) -> SourceResult {
    SourceResult::Failed {
        done: done.iter().map(|name| (*name).to_owned()).collect(),
        error: "storage unavailable".into(),
        permanent,
    }
}

fn results(entries: &[(&str, SourceResult)]) -> BTreeMap<String, SourceResult> {
    entries
        .iter()
        .map(|(source, result)| ((*source).to_owned(), result.clone()))
        .collect()
}

fn state_reading(source: &str, read_until: i64) -> RunState {
    RunState {
        sources: [(
            source.to_owned(),
            SourceState {
                read_until,
                ..SourceState::default()
            },
        )]
        .into(),
    }
}

#[test]
fn a_new_source_reads_the_lookback_up_to_the_delay() {
    let windows = RunState::default().windows(&sources(&["app"]), &timing(), NOW);

    assert_eq!(
        windows,
        vec![Window {
            source: "app".into(),
            start: NOW - 660 * S,
            end: NOW - 60 * S,
            done: BTreeSet::new(),
            attempt: 1,
        }]
    );
}

#[test]
fn each_window_starts_where_the_last_one_ended() {
    let names = sources(&["app"]);
    let first = RunState::default().windows(&names, &timing(), NOW);
    let (state, skipped) = RunState::default().after(
        &names,
        &first,
        &results(&[("app", SourceResult::Done)]),
        &timing(),
        RetryPolicy::default(),
        NOW,
    );

    assert!(skipped.is_empty());

    let second = state.windows(&names, &timing(), NOW + 300 * S);

    assert_eq!(second[0].start, first[0].end);
    assert_eq!(second[0].end, NOW + 240 * S);
    // Nothing new yet: no window, and nothing is due early.
    assert!(state.windows(&names, &timing(), NOW).is_empty());
    assert!(!state.due_early(&names, &timing(), NOW + 300 * S));
}

#[test]
fn a_source_far_behind_is_read_a_span_at_a_time_and_is_due_at_once() {
    let names = sources(&["app"]);
    let state = state_reading("app", NOW - 24 * 3_600 * S);

    let windows = state.windows(&names, &timing(), NOW);

    assert_eq!(windows[0].end - windows[0].start, timing().span());
    assert!(state.due_early(&names, &timing(), NOW));
}

#[test]
fn a_failed_window_keeps_its_place_and_is_retried_after_a_backoff() {
    let names = sources(&["app", "nginx"]);
    let windows = RunState::default().windows(&names, &timing(), NOW);
    let (state, _) = RunState::default().after(
        &names,
        &windows,
        &results(&[
            ("app", failed(&["clean"], false)),
            ("nginx", SourceResult::Done),
        ]),
        &timing(),
        RetryPolicy::default(),
        NOW,
    );

    // The other source goes on; the failed one waits 30 s with what it already wrote noted.
    assert_eq!(state.sources["nginx"].read_until, windows[1].end);
    let app = &state.sources["app"];
    assert_eq!(app.read_until, windows[0].start);
    let retry = app.retry.as_ref().unwrap();
    assert_eq!((retry.failures, retry.next_at), (1, NOW + 30 * S));
    assert_eq!(retry.until, windows[0].end);
    assert_eq!(retry.done, BTreeSet::from(["clean".to_owned()]));
    assert_eq!(state.pending_retry(), Some((1, NOW + 30 * S)));

    // Not before its time; then the same window, without what it already wrote.
    assert!(!state.due_early(&names, &timing(), NOW + 29 * S));
    assert!(state.due_early(&names, &timing(), NOW + 30 * S));
    let again = state.windows(&names, &timing(), NOW + 30 * S);
    let app_again = again.iter().find(|w| w.source == "app").unwrap();
    assert_eq!(
        (app_again.start, app_again.end),
        (windows[0].start, windows[0].end)
    );
    assert_eq!(app_again.attempt, 2);
    assert_eq!(app_again.done, retry.done);
}

#[test]
fn the_backoff_doubles_up_to_the_interval() {
    let timing = timing();

    assert_eq!(timing.backoff(1), 30 * S);
    assert_eq!(timing.backoff(2), 60 * S);
    assert_eq!(timing.backoff(3), 120 * S);
    assert_eq!(timing.backoff(4), 240 * S);
    assert_eq!(timing.backoff(5), 300 * S);
    assert_eq!(timing.backoff(40), 300 * S);
    // An interval shorter than the first backoff does not shorten it.
    assert_eq!(Timing::from_secs(10, 10, 0).backoff(3), 30 * S);
}

#[test]
fn when_the_retries_run_out_the_window_is_skipped_and_said_so() {
    let names = sources(&["app"]);
    let policy = RetryPolicy {
        run_retries: 1,
        delivery_retries: 0,
    };
    let windows = RunState::default().windows(&names, &timing(), NOW);
    let (state, skipped) = RunState::default().after(
        &names,
        &windows,
        &results(&[("app", failed(&[], false))]),
        &timing(),
        policy,
        NOW,
    );

    assert!(skipped.is_empty());

    let later = NOW + 30 * S;
    let retried = state.windows(&names, &timing(), later);
    let (state, skipped) = state.after(
        &names,
        &retried,
        &results(&[("app", failed(&[], false))]),
        &timing(),
        policy,
        later,
    );

    assert_eq!(state.sources["app"].read_until, windows[0].end);
    assert!(state.sources["app"].retry.is_none());
    assert_eq!(skipped.len(), 1);
    assert!(
        skipped[0].contains("gave up on `app` after 2 attempt(s)"),
        "{skipped:?}"
    );
}

#[test]
fn no_retries_means_a_failed_window_is_skipped_at_once() {
    let names = sources(&["app"]);
    let policy = RetryPolicy {
        run_retries: 0,
        delivery_retries: 0,
    };
    let windows = RunState::default().windows(&names, &timing(), NOW);

    let (state, skipped) = RunState::default().after(
        &names,
        &windows,
        &results(&[("app", failed(&[], false))]),
        &timing(),
        policy,
        NOW,
    );

    assert_eq!(state.sources["app"].read_until, windows[0].end);
    assert_eq!(skipped.len(), 1);
}

#[test]
fn a_configuration_failure_keeps_the_place_without_a_retry() {
    let names = sources(&["app"]);
    let state = state_reading("app", NOW - 600 * S);
    let windows = state.windows(&names, &timing(), NOW);

    let (next, skipped) = state.after(
        &names,
        &windows,
        &results(&[("app", failed(&[], true))]),
        &timing(),
        RetryPolicy::default(),
        NOW,
    );

    let app = &next.sources["app"];
    assert_eq!(app.read_until, NOW - 600 * S);
    assert_eq!(app.retry, None);
    assert_eq!(app.held.as_deref(), Some("storage unavailable"));
    assert!(skipped.is_empty());

    // Once it reads again, nothing holds it.
    let later = NOW + 300 * S;
    let windows = next.windows(&names, &timing(), later);
    let (next, _) = next.after(
        &names,
        &windows,
        &results(&[("app", SourceResult::Done)]),
        &timing(),
        RetryPolicy::default(),
        later,
    );

    assert_eq!(next.sources["app"].held, None);
}

/// Catching up at once would fail at every tick the way the last run did.
#[test]
fn a_held_source_far_behind_waits_for_the_regular_run() {
    let names = sources(&["app"]);
    let mut state = state_reading("app", NOW - 24 * 3_600 * S);

    assert!(state.due_early(&names, &timing(), NOW));

    state.sources.get_mut("app").unwrap().held = Some("invalid argument: bad query".into());

    assert!(!state.due_early(&names, &timing(), NOW));
    // The regular run still reads it, from where it stopped.
    assert_eq!(
        state.windows(&names, &timing(), NOW)[0].start,
        NOW - 24 * 3_600 * S
    );
}

#[test]
fn sources_no_longer_read_are_forgotten_and_unread_ones_kept() {
    let mut state = state_reading("app", NOW - 100 * S);
    state.sources.insert(
        "old".into(),
        SourceState {
            read_until: NOW,
            ..SourceState::default()
        },
    );

    let (next, _) = state.after(
        &sources(&["app"]),
        &[],
        &BTreeMap::new(),
        &timing(),
        RetryPolicy::default(),
        NOW,
    );

    assert_eq!(next, state_reading("app", NOW - 100 * S));
}

#[test]
fn the_state_reads_back_from_what_was_saved() {
    let mut state = state_reading("app", 42);
    state.sources.insert(
        "nginx".into(),
        SourceState {
            read_until: 7,
            retry: Some(Retry {
                failures: 2,
                next_at: 9,
                until: 8,
                done: BTreeSet::from(["clean".to_owned()]),
                error: "boom".into(),
            }),
            held: None,
        },
    );
    state.sources.insert(
        "audit".into(),
        SourceState {
            read_until: 3,
            retry: None,
            held: Some("forbidden".into()),
        },
    );

    assert_eq!(RunState::from_value(&state.to_value()), state);
    assert_eq!(RunState::from_value(&json!({})), RunState::default());
    assert_eq!(RunState::from_value(&json!("garbage")), RunState::default());
}
