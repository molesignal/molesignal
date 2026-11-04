// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What the model is told of a RUM application attached with `@`: how it did over the time the
//! question is about (the last day when it names none): the sessions and the share that met no
//! error and no crash, the Core Web Vitals of a browser application with their ratings, the issues
//! touching the most sessions, and the alerts going on for it. The figures are the ones the
//! overview page reads.

use std::fmt::Write as _;

use crate::{
    api::AppState,
    app::{
        iam::IamContext,
        rum::{
            issues::{
                self, Query,
                select::{Holder, IssueView, Sort, StatusFilter},
            },
            overview::{self, Overview},
            reader::{Reader, Span},
            vitals::Reading,
        },
    },
    domain::{alerting::incident::Incident, rum::RumApplication},
    shared::{
        Result,
        time::{TimeRange, TimestampMicros},
    },
};

const DAY_MICROS: i64 = 24 * 60 * 60 * 1_000_000;
/// How many issues, pages and alerts the model is told of.
const MOST_ISSUES: usize = 3;
const MOST_PAGES: usize = 3;
const MOST_ALERTS: usize = 5;

pub(super) async fn rum_application_facts(
    state: &AppState,
    ctx: &IamContext,
    application: &RumApplication,
    window: Option<TimeRange>,
) -> String {
    match facts(state, ctx, application, window).await {
        Ok(text) => text,
        Err(error) => format!(
            "RUM application: {} (id {}), platform {}\nIts figures could not be read just now: {error}",
            application.name,
            application.id,
            platform(application)
        ),
    }
}

fn platform(application: &RumApplication) -> String {
    serde_json::to_value(application.platform)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

async fn facts(
    state: &AppState,
    ctx: &IamContext,
    application: &RumApplication,
    window: Option<TimeRange>,
) -> Result<String> {
    let now = TimestampMicros::now().0;

    let span = window.map_or(
        Span {
            from: now - DAY_MICROS,
            to: now,
        },
        |window| Span {
            from: window.start.0,
            to: window.end.0,
        },
    );

    let reader = Reader::new(&state.query, state.telemetry.streams.as_ref(), ctx);

    let numbers = overview::read(&reader, application, None, span).await?;

    let query = Query {
        application: application.id.clone(),
        env: None,
        source: None,
        filters: Vec::new(),
        from: Some(span.from),
        to: Some(span.to),
        status: StatusFilter::Unresolved,
        assignee: Holder::Anyone,
        team: Holder::Anyone,
        search: String::new(),
        fresh: false,
        sort: Sort::Sessions,
        descending: true,
        limit: MOST_ISSUES,
    };

    let top = issues::list(&reader, state.telemetry.rum_issues.as_ref(), &query).await?;

    let going = if ctx.has_permission("alerts.rules.read") {
        going_on(state, ctx, application).await
    } else {
        Vec::new()
    };

    let mut text = format!(
        "RUM application: {} (id {}), platform {}\nFrom {} to {}.",
        application.name,
        application.id,
        platform(application),
        stamp(span.from),
        stamp(span.to),
    );

    let _ = write!(text, "\n{}", sessions_line(&numbers));

    if !numbers.vitals.is_empty() {
        let readings = numbers.vitals.iter().map(vital_text).collect::<Vec<_>>();

        let _ = write!(
            text,
            "\nCore Web Vitals, 75th percentile over {} views: {}.",
            numbers.views,
            readings.join(", ")
        );
    }

    if !numbers.pages.is_empty() {
        let pages = numbers
            .pages
            .iter()
            .take(MOST_PAGES)
            .map(|page| format!("{} ({} views)", page.page, page.views))
            .collect::<Vec<_>>();

        let _ = write!(text, "\nMost viewed: {}.", pages.join(", "));
    }

    if top.issues.is_empty() {
        let _ = write!(text, "\nNo unresolved issue had errors in the range.");
    } else {
        let _ = write!(
            text,
            "\nUnresolved issues touching the most sessions ({} in all):",
            top.total
        );

        for view in &top.issues {
            let _ = write!(text, "\n- {}", issue_line(view));
        }
    }

    if !going.is_empty() {
        let _ = write!(text, "\nAlerts going on:");

        for incident in going.iter().take(MOST_ALERTS) {
            let _ = write!(
                text,
                "\n- {:?} {} ({:?}, since {})",
                incident.severity,
                incident.summary,
                incident.status,
                stamp(incident.created_at.0)
            );
        }
    }

    if let Some(reason) = numbers.unavailable.or(top.unavailable) {
        let _ = write!(
            text,
            "\nSome figures are missing: counting did not finish ({}).",
            serde_json::to_value(reason)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
                .unwrap_or_default()
        );
    }

    Ok(text)
}

fn stamp(micros: i64) -> String {
    TimestampMicros(micros)
        .to_datetime()
        .format("%Y-%m-%d %H:%M UTC")
        .to_string()
}

fn percent(share: Option<f64>) -> String {
    share.map_or_else(|| "n/a".into(), |share| format!("{:.1}%", share * 100.0))
}

/// The sessions of the range and how many of them met an error, a crash or frustration.
fn sessions_line(numbers: &Overview) -> String {
    let pulse = &numbers.pulse;

    if pulse.sessions == 0 {
        return "No sessions in the range.".into();
    }

    format!(
        "{} sessions of {} people; {} without an error, {} without a crash; {} sessions showed frustration.",
        pulse.sessions,
        pulse.users,
        percent(pulse.error_free_share()),
        percent(pulse.crash_free_share()),
        pulse.frustrated
    )
}

/// A vital with its value in the words of its unit and its rating.
fn vital_text(reading: &Reading) -> String {
    let (name, value) = match reading.name {
        "lcp_ms" => ("LCP", seconds_or_millis(reading.p75)),
        "inp_ms" => ("INP", seconds_or_millis(reading.p75)),
        "ttfb_ms" => ("TTFB", seconds_or_millis(reading.p75)),
        _ => ("CLS", format!("{:.2}", reading.p75)),
    };

    format!("{name} {value} ({})", reading.rating.replace('_', " "))
}

fn seconds_or_millis(milliseconds: f64) -> String {
    if milliseconds >= 1_000.0 {
        format!("{:.1} s", milliseconds / 1_000.0)
    } else {
        format!("{milliseconds:.0} ms")
    }
}

fn issue_line(view: &IssueView) -> String {
    let issue = &view.issue;
    let numbers = view.numbers;

    let mut line = format!(
        "[{:?}] {}: {}",
        issue.status,
        issue.error_type,
        issue.message.chars().take(160).collect::<String>()
    );

    if !issue.location.is_empty() {
        let _ = write!(line, " at {}", issue.location);
    }

    let _ = write!(
        line,
        " — {} sessions, {} people, {} occurrences; id {}",
        numbers.sessions, numbers.users, numbers.occurrences, issue.id
    );

    if let Some(version) = &issue.first_version {
        let _ = write!(line, "; first seen in {version}");
    }

    if let Some(version) = &issue.regressed_version {
        let _ = write!(line, "; came back in {version}");
    }

    line
}

/// The incidents going on whose rule watches the application.
async fn going_on(
    state: &AppState,
    ctx: &IamContext,
    application: &RumApplication,
) -> Vec<Incident> {
    let mut incidents = state
        .alerting
        .service
        .incidents
        .list_active(&ctx.org_id)
        .await
        .unwrap_or_default();

    incidents.retain(|incident| {
        incident
            .labels
            .get("rum.application")
            .is_some_and(|id| *id == application.id)
    });

    // The worst first, and the newest first among equals.
    incidents.sort_by_key(|incident| {
        (
            std::cmp::Reverse(incident.severity),
            std::cmp::Reverse(incident.created_at.0),
        )
    });
    incidents
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::rum::overview::Pulse;

    #[test]
    fn a_vital_is_said_in_the_words_of_its_unit_with_its_rating() {
        let lcp = Reading {
            name: "lcp_ms",
            p75: 2_400.0,
            samples: 100,
            rating: "good",
        };
        let inp = Reading {
            name: "inp_ms",
            p75: 180.0,
            samples: 100,
            rating: "good",
        };
        let cls = Reading {
            name: "cls",
            p75: 0.123,
            samples: 100,
            rating: "needs_improvement",
        };

        assert_eq!(vital_text(&lcp), "LCP 2.4 s (good)");
        assert_eq!(vital_text(&inp), "INP 180 ms (good)");
        assert_eq!(vital_text(&cls), "CLS 0.12 (needs improvement)");
    }

    #[test]
    fn the_sessions_are_said_with_the_shares_that_met_nothing() {
        let numbers = Overview {
            pulse: Pulse {
                sessions: 200,
                users: 150,
                with_errors: 30,
                with_crashes: 2,
                users_with_crashes: 2,
                frustrated: 12,
                stored_sessions: 200,
            },
            vitals: Vec::new(),
            views: 0,
            pages: Vec::new(),
            unavailable: None,
        };

        assert_eq!(
            sessions_line(&numbers),
            "200 sessions of 150 people; 85.0% without an error, 99.0% without a crash; 12 sessions showed frustration."
        );

        let none = Overview {
            pulse: Pulse::default(),
            ..numbers
        };

        assert_eq!(sessions_line(&none), "No sessions in the range.");
    }
}
