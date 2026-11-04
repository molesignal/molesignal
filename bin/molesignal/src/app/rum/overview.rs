// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How an application did over a range, as the overview page reads it: how many sessions and
//! people there were and how many met an error, a crash or frustration; the Core Web Vitals of a
//! browser application; and the pages most viewed. The numbers come from the same aggregates the
//! page asks for, so what Mole Agent says is what the page shows.

use serde::Serialize;
use serde_json::Value;

use super::{
    aggregate::{self, Request, number},
    reader::{Reader, Span},
    selection::Selection,
    vitals::{self, Reading, is_core},
};
use crate::{
    domain::rum::{RumApplication, RumPlatform},
    infra::rum::query::{Aggregate, Filter, MeasureSpec, Unavailable},
    shared::Result,
};

/// How many pages the overview names.
const PAGES: usize = 5;

/// Where a view is told by: its URL's path with the changing parts grouped, or a screen's name.
fn page_field(platform: RumPlatform) -> &'static str {
    if platform == RumPlatform::Browser {
        "view.url_path_group"
    } else {
        "view.name"
    }
}

fn filter(field: &str, op: &str, value: &str) -> Filter {
    Filter {
        field: field.into(),
        op: op.into(),
        value: value.into(),
        quoted: false,
    }
}

/// The filters every read of an application carries: the application, and the environment if one
/// is chosen.
pub fn scope_filters(application: &str, environment: Option<&str>) -> Vec<Filter> {
    let mut filters = vec![filter("application.id", "=", application)];

    if let Some(environment) = environment
        .map(str::trim)
        .filter(|environment| !environment.is_empty())
    {
        filters.push(filter("env", "=", environment));
    }

    filters
}

fn positive(field: &str) -> Vec<Filter> {
    vec![filter(field, ">", "0")]
}

fn counted(name: &str, filters: Vec<Filter>) -> MeasureSpec {
    MeasureSpec {
        function: "count".into(),
        field: None,
        fallback: None,
        filters,
        name: Some(name.into()),
    }
}

/// The rows that are stored, whatever they stand for.
fn stored(name: &str) -> MeasureSpec {
    MeasureSpec {
        function: "stored".into(),
        field: None,
        fallback: None,
        filters: Vec::new(),
        name: Some(name.into()),
    }
}

/// People are told apart by their id, or where a visitor has none by the anonymous id the SDK
/// keeps.
fn people(name: &str, filters: Vec<Filter>) -> MeasureSpec {
    MeasureSpec {
        function: "count_distinct".into(),
        field: Some("usr.id".into()),
        fallback: Some("usr.anonymous_id".into()),
        filters,
        name: Some(name.into()),
    }
}

/// What the sessions of a range came to. An application that keeps only a share of its sessions
/// has them counted for as many as the share they were kept with says; `stored_sessions` is how
/// many of them are stored, and the numbers are estimates when it is under `sessions`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pulse {
    pub sessions: u64,
    pub stored_sessions: u64,
    pub users: u64,
    /// Sessions with at least one error.
    pub with_errors: u64,
    /// Sessions with at least one crash.
    pub with_crashes: u64,
    pub users_with_crashes: u64,
    /// Sessions with at least one sign of frustration.
    pub frustrated: u64,
}

impl Pulse {
    /// How many sessions and people there were, and how many met an error, a crash or frustration.
    fn measures() -> Vec<MeasureSpec> {
        vec![
            counted("sessions", Vec::new()),
            people("users", Vec::new()),
            counted("with_errors", positive("session.error.count")),
            counted("with_crashes", positive("session.crash.count")),
            people("users_with_crashes", positive("session.crash.count")),
            counted("frustrated", positive("session.frustration.count")),
            stored("stored_sessions"),
        ]
    }

    fn of(values: &[Value]) -> Self {
        Self {
            sessions: number(values.first()),
            users: number(values.get(1)),
            with_errors: number(values.get(2)),
            with_crashes: number(values.get(3)),
            users_with_crashes: number(values.get(4)),
            frustrated: number(values.get(5)),
            stored_sessions: number(values.get(6)),
        }
    }

    /// The share of the sessions with none of what `affected` counts; none where there were no
    /// sessions.
    fn free_of(&self, affected: u64) -> Option<f64> {
        (self.sessions > 0)
            .then(|| 1.0 - (affected.min(self.sessions) as f64 / self.sessions as f64))
    }

    pub fn error_free_share(&self) -> Option<f64> {
        self.free_of(self.with_errors)
    }

    pub fn crash_free_share(&self) -> Option<f64> {
        self.free_of(self.with_crashes)
    }
}

/// A page (a screen in an app) and how its views did.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub page: String,
    pub views: u64,
    /// Views that met an error.
    pub with_errors: u64,
    pub vitals: Vec<Reading>,
}

#[derive(Debug, Serialize)]
pub struct Overview {
    pub pulse: Pulse,
    /// The Core Web Vitals over every view; none for an application that is not a browser's.
    pub vitals: Vec<Reading>,
    pub views: u64,
    /// The most viewed pages, the most viewed first.
    pub pages: Vec<Page>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<Unavailable>,
}

fn core(values: &[Value]) -> Vec<Reading> {
    vitals::readings(values)
        .into_iter()
        .filter(|reading| is_core(reading.name))
        .collect()
}

async fn ask(
    reader: &Reader<'_>,
    stream: &str,
    span: Span,
    filters: &[Filter],
    measures: &[MeasureSpec],
    group_by: &[String],
    limit: Option<usize>,
) -> Result<Aggregate> {
    let selection = Selection::new(
        Some(stream),
        Some(span.from),
        Some(span.to),
        filters.to_vec(),
        Vec::new(),
    )?;

    aggregate::run(
        reader,
        &Request {
            selection: &selection,
            measures,
            group_by,
            bucket_micros: None,
            origin_micros: None,
            // The groups are ranked by the first measure, the views of a page.
            order_by: None,
            descending: None,
            limit,
            estimate: true,
        },
    )
    .await
}

/// How the application did over the range, in one environment or in all of them.
pub async fn read(
    reader: &Reader<'_>,
    application: &RumApplication,
    environment: Option<&str>,
    span: Span,
) -> Result<Overview> {
    let browser = application.platform == RumPlatform::Browser;
    let filters = scope_filters(&application.id, environment);

    let sessions = ask(
        reader,
        "sessions",
        span,
        &filters,
        &Pulse::measures(),
        &[],
        None,
    )
    .await?;

    let mut unavailable = sessions.unavailable;

    let pulse = sessions
        .rows
        .first()
        .map(|row| Pulse::of(&row.values))
        .unwrap_or_default();

    // Every view of the range: its count, and for a browser the vitals over all of them.
    let mut overall = Vec::new();
    let mut views = 0;

    let everything = if browser {
        vitals::measures()
    } else {
        vec![counted("views", Vec::new())]
    };

    let answer = ask(reader, "views", span, &filters, &everything, &[], None).await?;

    unavailable = unavailable.or(answer.unavailable);

    if let Some(row) = answer.rows.first() {
        views = number(row.values.first());

        if browser {
            overall = core(&row.values);
        }
    }

    // Views are told apart by their page; the vitals lead the measures the way they do in `vitals`.
    let mut measures = if browser {
        vitals::measures()
    } else {
        vec![counted("views", Vec::new())]
    };

    measures.push(counted("with_errors", positive("view.error.count")));

    let answer = ask(
        reader,
        "views",
        span,
        &filters,
        &measures,
        &[page_field(application.platform).to_string()],
        Some(PAGES),
    )
    .await?;

    unavailable = unavailable.or(answer.unavailable);

    let pages = answer
        .rows
        .iter()
        .filter_map(|row| {
            let page = row
                .group
                .first()?
                .as_deref()
                .filter(|page| !page.is_empty())?;

            Some(Page {
                page: page.to_string(),
                views: number(row.values.first()),
                with_errors: number(row.values.get(measures.len() - 1)),
                vitals: if browser {
                    core(&row.values)
                } else {
                    Vec::new()
                },
            })
        })
        .collect();

    Ok(Overview {
        pulse,
        vitals: overall,
        views,
        pages,
        unavailable,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_pulse_is_read_in_the_order_of_its_measures() {
        let measures = Pulse::measures()
            .into_iter()
            .map(|measure| measure.name.expect("a name"))
            .collect::<Vec<_>>();

        assert_eq!(
            measures,
            [
                "sessions",
                "users",
                "with_errors",
                "with_crashes",
                "users_with_crashes",
                "frustrated",
                "stored_sessions"
            ]
        );

        let pulse = Pulse::of(&[
            json!(200),
            json!(150),
            json!(30),
            json!(4),
            json!(3),
            json!(12),
            json!(50),
        ]);

        assert_eq!(pulse.sessions, 200);
        assert_eq!(pulse.stored_sessions, 50);
        assert_eq!(pulse.frustrated, 12);
        assert!((pulse.error_free_share().unwrap() - 0.85).abs() < 1e-9);
        assert!((pulse.crash_free_share().unwrap() - 0.98).abs() < 1e-9);
    }

    #[test]
    fn no_sessions_have_no_share() {
        assert_eq!(Pulse::default().error_free_share(), None);
        assert_eq!(Pulse::of(&[]).crash_free_share(), None);
    }

    #[test]
    fn the_scope_is_the_application_and_the_environment_if_one_is_chosen() {
        let all = scope_filters("shop-web", None);

        assert_eq!(all.len(), 1);
        assert_eq!(all[0].field, "application.id");
        assert_eq!(scope_filters("shop-web", Some("  ")).len(), 1);

        let prod = scope_filters("shop-web", Some(" prod "));

        assert_eq!(prod[1].field, "env");
        assert_eq!(prod[1].value, "prod");
    }

    #[test]
    fn a_page_is_a_path_group_in_a_browser_and_a_name_in_an_app() {
        assert_eq!(page_field(RumPlatform::Browser), "view.url_path_group");
        assert_eq!(page_field(RumPlatform::Ios), "view.name");
    }

    #[test]
    fn only_the_core_vitals_are_named() {
        let values = [
            json!(10),
            json!(1_000_000_000.0),
            json!(10),
            json!(100_000_000.0),
            json!(10),
            json!(0.05),
            json!(10),
            json!(500_000_000.0),
            json!(10),
        ];

        assert_eq!(
            core(&values)
                .iter()
                .map(|reading| reading.name)
                .collect::<Vec<_>>(),
            ["lcp_ms", "inp_ms", "cls"]
        );
    }
}
