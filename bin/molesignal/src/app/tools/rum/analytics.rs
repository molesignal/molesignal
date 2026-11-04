// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What people do across a session, in words: how many sessions got through a series of steps,
//! where they went from a page or before it, and who came back. The statements are the ones the
//! analytics tab runs, so the numbers agree with the funnel, the paths and the retention it shows.

use chrono::DateTime;
use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::ToolResult;

use super::{reader, summary_span};
use crate::{
    app::{
        iam::IamContext,
        rum::{
            analytics::{self, Answer, FunnelRequest, PathsRequest, RetentionRequest},
            overview::scope_filters,
        },
        tools::{
            ToolRuntime,
            common::{TimeRangeArg, parse_args, time_range},
        },
    },
    domain::rum::{RumApplication, RumPlatform},
    infra::rum::{
        analytics::{Funnel, OTHER, Paths, Period, Retention, Step, StepKind},
        query::Filter,
    },
    shared::{Error, Result},
};

/// A share of a whole, as a percentage with one decimal.
fn percent(share: Option<f64>) -> Value {
    share.map_or(Value::Null, |share| json!((share * 1_000.0).round() / 10.0))
}

fn share(part: u64, whole: u64) -> Option<f64> {
    (whole > 0).then(|| part as f64 / whole as f64)
}

/// Milliseconds as seconds with one decimal.
fn seconds(milliseconds: f64) -> f64 {
    (milliseconds / 100.0).round() / 10.0
}

fn environment_of(value: Option<&str>) -> Option<&str> {
    value
        .map(str::trim)
        .filter(|environment| !environment.is_empty())
}

/// The application a question is about, if it exists.
async fn find(
    runtime: &ToolRuntime,
    auth: &IamContext,
    id: &str,
) -> Result<Option<RumApplication>> {
    match runtime
        .observability
        .rum_applications
        .get(&auth.org_id, id.trim())
        .await
    {
        Ok(application) => Ok(Some(application)),
        Err(Error::NotFound(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

fn not_found(id: &str) -> ToolResult {
    ToolResult::error(format!("RUM application {} not found", id.trim()))
}

fn about(application: &RumApplication, environment: Option<&str>) -> Value {
    json!({
        "id": application.id,
        "name": application.name,
        "platform": application.platform,
        "environment": environment,
    })
}

fn range_of(answer_from: i64, answer_to: i64) -> Value {
    json!({ "startTime": answer_from, "endTime": answer_to })
}

/// One step of a funnel as the model says it: what to look for, and in which field.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StepArg {
    #[serde(rename = "type")]
    kind: StepKind,
    #[serde(default)]
    field: Option<String>,
    #[serde(default)]
    op: Option<String>,
    value: String,
    #[serde(default)]
    name: Option<String>,
}

/// The field a step looks in unless it says: the page of a browser, the screen of an app, the
/// element of an action.
fn default_field(kind: StepKind, browser: bool) -> &'static str {
    match (kind, browser) {
        (StepKind::Actions, _) => "action.target.name",
        (StepKind::Views, true) => "view.url_path_group",
        (StepKind::Views, false) => "view.name",
    }
}

impl StepArg {
    fn into_step(self, browser: bool) -> Result<Step> {
        let value = self.value.trim().to_string();

        if value.is_empty() {
            return Err(Error::invalid("every step needs a value to look for"));
        }

        let field = self
            .field
            .as_deref()
            .map(str::trim)
            .filter(|field| !field.is_empty())
            .unwrap_or_else(|| default_field(self.kind, browser))
            .to_string();

        let op = match self.op.as_deref().map(str::trim) {
            None | Some("" | "=") => "=",
            Some("contains") => "contains",
            Some(other) => {
                return Err(Error::invalid(format!(
                    "op must be = or contains, not `{other}`"
                )));
            }
        };

        Ok(Step {
            kind: self.kind,
            name: self.name.filter(|name| !name.trim().is_empty()),
            filters: vec![Filter {
                field,
                op: op.into(),
                value,
                quoted: false,
            }],
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FunnelArgs {
    application: String,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    steps: Vec<StepArg>,
    #[serde(default)]
    window_seconds: Option<i64>,
    #[serde(default)]
    breakdown: Option<String>,
}

pub(super) async fn funnel(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: FunnelArgs = parse_args(arguments)?;
    let span = summary_span(args.time_range)?;

    let Some(application) = find(runtime, auth, &args.application).await? else {
        return Ok(not_found(&args.application));
    };

    let environment = environment_of(args.environment.as_deref());
    let browser = application.platform == RumPlatform::Browser;

    let steps = args
        .steps
        .into_iter()
        .map(|step| step.into_step(browser))
        .collect::<Result<Vec<_>>>()?;

    let answer = analytics::funnel(
        &reader(runtime, auth),
        &FunnelRequest {
            from: Some(span.from),
            to: Some(span.to),
            filters: scope_filters(&application.id, environment),
            steps,
            window_secs: args.window_seconds,
            breakdown: args.breakdown.clone(),
        },
    )
    .await?;

    Ok(ToolResult::json(describe_funnel(
        &application,
        environment,
        args.breakdown.as_deref(),
        &answer,
    )))
}

/// What a funnel comes to: for each step the sessions that got there, as shares of the first
/// step's and of the step before's, and the median time since the step before.
fn describe_funnel(
    application: &RumApplication,
    environment: Option<&str>,
    breakdown: Option<&str>,
    answer: &Answer<Funnel>,
) -> Value {
    let groups = answer
        .body
        .groups
        .iter()
        .map(|group| {
            let steps = group
                .steps
                .iter()
                .enumerate()
                .map(|(index, step)| {
                    json!({
                        "step": index + 1,
                        "name": step.name,
                        "sessions": step.sessions,
                        "ofFirstStepPercent": percent(step.conversion),
                        "ofPreviousStepPercent": percent(step.from_previous),
                        "medianSecondsFromPreviousStep": step.median_ms.map(seconds),
                    })
                })
                .collect::<Vec<_>>();

            json!({ "value": group.value, "steps": steps })
        })
        .collect::<Vec<_>>();

    let mut described = json!({
        "application": about(application, environment),
        "range": range_of(answer.from, answer.to),
        "breakdown": breakdown,
        "groups": groups,
    });

    if let Some(reason) = &answer.unavailable {
        described["unavailable"] = json!(reason);
    }

    described
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PathsArgs {
    application: String,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    page: Option<String>,
    #[serde(default)]
    direction: Option<String>,
    #[serde(default)]
    depth: Option<usize>,
    #[serde(default)]
    page_field: Option<String>,
}

/// Whether the paths go back from the page, as the model says.
fn backward(direction: Option<&str>) -> Result<bool> {
    match direction.map(str::trim) {
        None | Some("" | "after") => Ok(false),
        Some("before") => Ok(true),
        Some(other) => Err(Error::invalid(format!(
            "direction must be after or before, not `{other}`"
        ))),
    }
}

pub(super) async fn paths(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: PathsArgs = parse_args(arguments)?;
    let span = summary_span(args.time_range)?;
    let backward = backward(args.direction.as_deref())?;

    let Some(application) = find(runtime, auth, &args.application).await? else {
        return Ok(not_found(&args.application));
    };

    let environment = environment_of(args.environment.as_deref());
    let anchor = args
        .page
        .as_deref()
        .map(str::trim)
        .filter(|page| !page.is_empty());

    let by = args.page_field.clone().unwrap_or_else(|| {
        default_field(
            StepKind::Views,
            application.platform == RumPlatform::Browser,
        )
        .into()
    });

    let answer = analytics::paths(
        &reader(runtime, auth),
        &PathsRequest {
            from: Some(span.from),
            to: Some(span.to),
            filters: scope_filters(&application.id, environment),
            anchor: anchor.map(str::to_string),
            backward,
            depth: args.depth,
            by: Some(by),
        },
    )
    .await?;

    Ok(ToolResult::json(describe_paths(
        &application,
        environment,
        anchor,
        &answer,
    )))
}

/// What the paths come to. A page is said by how many pages it is from the one the paths are of
/// (0 is that page; going back, the pages before it), and a link always goes from the page that
/// was opened first to the one opened next.
fn describe_paths(
    application: &RumApplication,
    environment: Option<&str>,
    anchor: Option<&str>,
    answer: &Answer<Paths>,
) -> Value {
    let paths = &answer.body;
    let last = paths.columns.saturating_sub(1);

    let away = |column: usize| {
        if paths.backward {
            last - column
        } else {
            column
        }
    };
    let name = |page: &str| {
        if page == OTHER {
            "(other pages)".to_string()
        } else {
            page.to_string()
        }
    };

    let mut nodes = paths.nodes.iter().collect::<Vec<_>>();

    nodes.sort_by(|left, right| {
        away(left.column)
            .cmp(&away(right.column))
            .then_with(|| (left.page == OTHER).cmp(&(right.page == OTHER)))
            .then_with(|| right.sessions.cmp(&left.sessions))
            .then_with(|| left.page.cmp(&right.page))
    });

    let ending = if paths.backward {
        "beganHere"
    } else {
        "endedHere"
    };

    let pages = nodes
        .iter()
        .map(|node| {
            json!({
                "pagesAway": away(node.column),
                "page": name(&node.page),
                "sessions": node.sessions,
                (ending): node.ends,
            })
        })
        .collect::<Vec<_>>();

    let links = paths
        .links
        .iter()
        .map(|link| {
            json!({
                "from": name(&link.source),
                "fromPagesAway": away(link.column),
                "to": name(&link.target),
                "sessions": link.sessions,
            })
        })
        .collect::<Vec<_>>();

    let mut described = json!({
        "application": about(application, environment),
        "range": range_of(answer.from, answer.to),
        "direction": if paths.backward { "before" } else { "after" },
        "page": anchor,
        "sessions": paths.sessions,
        "pages": pages,
        "links": links,
    });

    if let Some(reason) = &answer.unavailable {
        described["unavailable"] = json!(reason);
    }

    described
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RetentionArgs {
    application: String,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    period: Option<Period>,
    #[serde(default)]
    periods: Option<usize>,
}

pub(super) async fn retention(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: RetentionArgs = parse_args(arguments)?;

    // Without a range the question looks back far enough to have cohorts.
    let (from, to) = match args.time_range {
        Some(value) => {
            let range = time_range(value)?;

            (Some(range.start.0), Some(range.end.0))
        }
        None => (None, None),
    };

    let Some(application) = find(runtime, auth, &args.application).await? else {
        return Ok(not_found(&args.application));
    };

    let environment = environment_of(args.environment.as_deref());

    let answer = analytics::retention(
        &reader(runtime, auth),
        &RetentionRequest {
            from,
            to,
            filters: scope_filters(&application.id, environment),
            period: args.period,
            periods: args.periods,
        },
    )
    .await?;

    Ok(ToolResult::json(describe_retention(
        &application,
        environment,
        &answer,
    )))
}

/// What retention comes to: for each cohort (the people first seen in a UTC day or week) how many
/// were seen in each period after, and the share they are of it; a period that has not ended
/// by the end of the range says nothing. The average weighs each cohort by its people.
fn describe_retention(
    application: &RumApplication,
    environment: Option<&str>,
    answer: &Answer<Retention>,
) -> Value {
    let retention = &answer.body;

    let cohorts = retention
        .cohorts
        .iter()
        .map(|cohort| {
            let begins = DateTime::from_timestamp_micros(cohort.start_time)
                .map(|moment| moment.format("%Y-%m-%d").to_string());

            let percents = cohort
                .returned
                .iter()
                .map(|count| count.map(|count| percent(share(count, cohort.users))))
                .collect::<Vec<_>>();

            json!({
                "begins": begins,
                "users": cohort.users,
                "returned": cohort.returned,
                "returnedPercent": percents,
            })
        })
        .collect::<Vec<_>>();

    let average = (0..retention.periods)
        .map(|index| {
            let (returned, users) = retention
                .cohorts
                .iter()
                .filter_map(|cohort| {
                    cohort
                        .returned
                        .get(index)
                        .copied()
                        .flatten()
                        .map(|count| (count, cohort.users))
                })
                .fold((0, 0), |(returned, users), (count, people)| {
                    (returned + count, users + people)
                });

            percent(share(returned, users))
        })
        .collect::<Vec<_>>();

    let period = match retention.period {
        Period::Day => "day",
        Period::Week => "week",
    };

    let mut described = json!({
        "application": about(application, environment),
        "range": range_of(answer.from, answer.to),
        "period": period,
        "periods": retention.periods,
        "cohorts": cohorts,
        "averageReturnedPercent": average,
    });

    if let Some(reason) = &answer.unavailable {
        described["unavailable"] = json!(reason);
    }

    described
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::rum::RumApplicationSettings,
        infra::rum::analytics::{Cohort, FunnelGroup, FunnelStep, PathLink, PathNode},
        shared::{ids::Id, time::TimestampMicros},
    };

    fn application(platform: RumPlatform) -> RumApplication {
        RumApplication {
            org_id: Id::from_string("org"),
            id: "shop-web".into(),
            name: "Shop".into(),
            platform,
            settings: RumApplicationSettings::default(),
            created_by: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn answered<T: serde::Serialize>(body: T) -> Answer<T> {
        Answer {
            body,
            from: 10,
            to: 20,
            unavailable: None,
        }
    }

    fn step(value: Value) -> StepArg {
        serde_json::from_value(value).expect("step")
    }

    #[test]
    fn a_step_looks_for_a_page_by_its_group_and_for_an_action_by_its_name_unless_it_says() {
        let page = step(json!({ "type": "views", "value": " /checkout " }))
            .into_step(true)
            .expect("step");

        assert_eq!(page.kind, StepKind::Views);
        assert_eq!(page.filters[0].field, "view.url_path_group");
        assert_eq!(page.filters[0].op, "=");
        assert_eq!(page.filters[0].value, "/checkout");

        let screen = step(json!({ "type": "views", "value": "Cart" }))
            .into_step(false)
            .expect("step");

        assert_eq!(screen.filters[0].field, "view.name");

        let action =
            step(json!({ "type": "actions", "value": "Pay", "op": "contains", "name": "Paid" }))
                .into_step(true)
                .expect("step");

        assert_eq!(action.filters[0].field, "action.target.name");
        assert_eq!(action.filters[0].op, "contains");
        assert_eq!(action.name.as_deref(), Some("Paid"));
    }

    #[test]
    fn a_step_names_the_field_it_looks_in_and_has_to_have_something_to_look_for() {
        let exact = step(json!({ "type": "views", "field": "view.url_path", "value": "/a/1" }))
            .into_step(true)
            .expect("step");

        assert_eq!(exact.filters[0].field, "view.url_path");

        assert!(
            step(json!({ "type": "views", "value": "  " }))
                .into_step(true)
                .is_err()
        );
        assert!(
            step(json!({ "type": "views", "value": "/a", "op": "like" }))
                .into_step(true)
                .is_err()
        );
        assert!(
            serde_json::from_value::<StepArg>(json!({ "type": "clicks", "value": "a" })).is_err()
        );
        assert!(
            serde_json::from_value::<StepArg>(json!({ "type": "views", "value": "a", "x": 1 }))
                .is_err()
        );
    }

    #[test]
    fn the_direction_is_after_unless_it_is_before() {
        assert!(!backward(None).expect("after"));
        assert!(!backward(Some("after")).expect("after"));
        assert!(backward(Some(" before ")).expect("before"));
        assert!(backward(Some("sideways")).is_err());
    }

    #[test]
    fn a_funnel_says_each_step_by_the_sessions_that_got_there_and_how_long_they_took() {
        let funnel = Funnel {
            groups: vec![FunnelGroup {
                value: None,
                steps: vec![
                    FunnelStep {
                        name: "/cart".into(),
                        sessions: 4,
                        conversion: Some(1.0),
                        from_previous: None,
                        median_ms: None,
                    },
                    FunnelStep {
                        name: "/checkout".into(),
                        sessions: 1,
                        conversion: Some(0.25),
                        from_previous: Some(0.25),
                        median_ms: Some(210_000.0),
                    },
                ],
            }],
        };

        let said = describe_funnel(
            &application(RumPlatform::Browser),
            Some("prod"),
            None,
            &answered(funnel),
        );

        assert_eq!(said["application"]["environment"], "prod");
        assert_eq!(said["range"]["endTime"], 20);

        let steps = &said["groups"][0]["steps"];

        assert_eq!(steps[0]["step"], 1);
        assert_eq!(steps[0]["medianSecondsFromPreviousStep"], Value::Null);
        assert_eq!(steps[1]["sessions"], 1);
        assert_eq!(steps[1]["ofFirstStepPercent"], 25.0);
        assert_eq!(steps[1]["medianSecondsFromPreviousStep"], 210.0);
        assert!(said.get("unavailable").is_none());
    }

    #[test]
    fn paths_say_how_far_a_page_is_from_the_one_they_are_of_and_go_from_the_earlier_page_to_the_later()
     {
        let paths = Paths {
            columns: 3,
            sessions: 5,
            backward: true,
            nodes: vec![
                PathNode {
                    column: 0,
                    page: "/products".into(),
                    sessions: 3,
                    ends: 3,
                },
                PathNode {
                    column: 1,
                    page: "/cart".into(),
                    sessions: 5,
                    ends: 0,
                },
                PathNode {
                    column: 2,
                    page: "/checkout".into(),
                    sessions: 5,
                    ends: 0,
                },
                PathNode {
                    column: 0,
                    page: OTHER.into(),
                    sessions: 2,
                    ends: 2,
                },
            ],
            links: vec![PathLink {
                column: 1,
                source: "/cart".into(),
                target: "/checkout".into(),
                sessions: 5,
            }],
        };

        let said = describe_paths(
            &application(RumPlatform::Browser),
            None,
            Some("/checkout"),
            &answered(paths),
        );

        assert_eq!(said["direction"], "before");
        assert_eq!(said["page"], "/checkout");

        let pages = said["pages"].as_array().expect("pages");

        // The page the paths are of first, then the pages before it, the most visited first.
        assert_eq!(pages[0]["page"], "/checkout");
        assert_eq!(pages[0]["pagesAway"], 0);
        assert_eq!(pages[1]["page"], "/cart");
        assert_eq!(pages[1]["pagesAway"], 1);
        assert_eq!(pages[2]["page"], "/products");
        assert_eq!(pages[2]["pagesAway"], 2);
        assert_eq!(pages[2]["beganHere"], 3);
        assert_eq!(pages[3]["page"], "(other pages)");

        assert_eq!(said["links"][0]["from"], "/cart");
        assert_eq!(said["links"][0]["fromPagesAway"], 1);
        assert_eq!(said["links"][0]["to"], "/checkout");
    }

    #[test]
    fn retention_says_each_cohort_by_its_day_and_averages_by_the_people_of_the_cohorts() {
        let day = 86_400_000_000;
        let retention = Retention {
            period: Period::Day,
            periods: 2,
            cohorts: vec![
                Cohort {
                    start_time: 20_595 * day,
                    users: 1,
                    returned: vec![Some(1), Some(0)],
                },
                Cohort {
                    start_time: 20_596 * day,
                    users: 6,
                    returned: vec![Some(0), None],
                },
            ],
        };

        let said = describe_retention(
            &application(RumPlatform::Browser),
            None,
            &answered(retention),
        );

        assert_eq!(said["period"], "day");
        assert_eq!(said["cohorts"][0]["begins"], "2026-05-22");
        assert_eq!(said["cohorts"][0]["returnedPercent"][0], 100.0);
        assert_eq!(said["cohorts"][1]["returnedPercent"][1], Value::Null);

        // One of seven came back after a day; none of the one cohort that could say after two.
        assert_eq!(said["averageReturnedPercent"][0], 14.3);
        assert_eq!(said["averageReturnedPercent"][1], 0.0);
    }
}
