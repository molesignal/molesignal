// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What frustrates the users of a RUM application: how many actions showed each sign and in how
//! many sessions, and the groups of them (the elements, the pages, the versions, the browsers or
//! systems) by the sessions each touched. An SDK judges the signs where the person is; the server
//! does not judge them again.

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::ToolResult;

use super::{reader, summary_span};
use crate::{
    app::{
        iam::IamContext,
        rum::{
            aggregate::{self, Request, number},
            overview::scope_filters,
            reader::{Reader, Span},
            selection::Selection,
        },
        tools::{
            ToolRuntime,
            common::{TimeRangeArg, parse_args},
        },
    },
    domain::rum::{RumApplication, RumPlatform},
    infra::rum::query::{Aggregate, Filter, MeasureSpec},
    shared::{Error, Result},
};

const COUNT_FIELD: &str = "action.frustration.count";
const TYPE_FIELD: &str = "action.frustration.type";
const SESSION: &str = "session.id";

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 50;

/// The three signs an SDK reads: clicks again and again, a click nothing came of, a click that
/// ended in an error. A browser writes `rage_click`, an app `rage_tap`: the beginning tells the
/// kind of both.
const KINDS: [(&str, &str); 3] = [("rage", "rage_"), ("dead", "dead_"), ("error", "error_")];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Args {
    application: String,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    group_by: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

fn filter(field: &str, op: &str, value: &str) -> Filter {
    Filter {
        field: field.into(),
        op: op.into(),
        value: value.into(),
        quoted: false,
    }
}

fn prefix_of(kind: &str) -> Option<&'static str> {
    KINDS
        .iter()
        .find(|(name, _)| *name == kind)
        .map(|(_, prefix)| *prefix)
}

fn of_kind(prefix: &str) -> Vec<Filter> {
    vec![filter(TYPE_FIELD, "contains", prefix)]
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

/// People who met a thing are counted by their sessions: one who clicks ten times is not ten.
fn sessions_of(name: &str, filters: Vec<Filter>) -> MeasureSpec {
    MeasureSpec {
        function: "count_distinct".into(),
        field: Some(SESSION.into()),
        fallback: None,
        filters,
        name: Some(name.into()),
    }
}

/// The columns the table is of: the values of one or two of them make a group.
fn fields_of(platform: RumPlatform, group: &str) -> Result<Vec<&'static str>> {
    let browser = platform == RumPlatform::Browser;

    Ok(match group {
        "element" if browser => vec!["action.target.name", "action.target.selector"],
        "element" => vec!["action.target.name"],
        "page" if browser => vec!["view.url_path_group"],
        "page" => vec!["view.name"],
        "version" => vec!["version"],
        "browser" => vec!["browser.name"],
        "system" => vec!["os.name"],
        other => {
            return Err(Error::invalid(format!(
                "group_by must be element, page, version, browser or system, not `{other}`"
            )));
        }
    })
}

async fn ask(
    reader: &Reader<'_>,
    stream: &str,
    span: Span,
    filters: Vec<Filter>,
    measures: &[MeasureSpec],
    group_by: &[String],
    limit: Option<usize>,
) -> Result<Aggregate> {
    let selection = Selection::new(
        Some(stream),
        Some(span.from),
        Some(span.to),
        filters,
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
            order_by: group_by.first().map(|_| "sessions"),
            descending: Some(true),
            limit,
            estimate: true,
        },
    )
    .await
}

pub(super) async fn summarize(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: Args = parse_args(arguments)?;
    let span = summary_span(args.time_range)?;

    let application = match runtime
        .observability
        .rum_applications
        .get(&auth.org_id, args.application.trim())
        .await
    {
        Ok(application) => application,
        Err(Error::NotFound(_)) => {
            return Ok(ToolResult::error(format!(
                "RUM application {} not found",
                args.application.trim()
            )));
        }
        Err(error) => return Err(error),
    };

    let kind = args
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|kind| !kind.is_empty());

    let prefix = kind
        .map(|kind| {
            prefix_of(kind).ok_or_else(|| {
                Error::invalid(format!("kind must be rage, dead or error, not `{kind}`"))
            })
        })
        .transpose()?;

    let group = args.group_by.as_deref().unwrap_or("element").trim();
    let fields = fields_of(application.platform, group)?;
    let limit = args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    let reader = reader(runtime, auth);

    let mut answer = read(
        &reader,
        &application,
        args.environment.as_deref(),
        span,
        prefix,
        &fields,
        limit,
    )
    .await?;

    answer["groupBy"] = json!(group);
    answer["kind"] = json!(kind);
    answer["range"] = json!({ "startTime": span.from, "endTime": span.to });

    Ok(ToolResult::json(answer))
}

/// The three reads behind the page: what the signs came to, the share of the sessions that showed
/// one, and the table of groups.
async fn read(
    reader: &Reader<'_>,
    application: &RumApplication,
    environment: Option<&str>,
    span: Span,
    prefix: Option<&str>,
    fields: &[&str],
    limit: usize,
) -> Result<Value> {
    let scope = scope_filters(&application.id, environment);
    let frustrated = [filter(COUNT_FIELD, ">", "0")];

    let acted = |extra: Vec<Filter>| {
        let mut filters = scope.clone();

        filters.extend(frustrated.iter().cloned());
        filters.extend(extra);
        filters
    };

    let mut statement = vec![sessions_of("sessions", Vec::new())];

    for (kind, prefix) in KINDS {
        statement.push(counted(kind, of_kind(prefix)));
        statement.push(sessions_of(&format!("{kind}_sessions"), of_kind(prefix)));
    }

    let said = ask(
        reader,
        "actions",
        span,
        acted(Vec::new()),
        &statement,
        &[],
        None,
    )
    .await?;

    let share = ask(
        reader,
        "sessions",
        span,
        scope.clone(),
        &[
            counted("sessions", Vec::new()),
            counted(
                "affected",
                vec![filter("session.frustration.count", ">", "0")],
            ),
        ],
        &[],
        None,
    )
    .await?;

    let group_by = fields
        .iter()
        .map(|field| field.to_string())
        .collect::<Vec<_>>();

    let mut table = KINDS
        .iter()
        .map(|(kind, prefix)| counted(kind, of_kind(prefix)))
        .collect::<Vec<_>>();

    table.push(sessions_of("sessions", Vec::new()));

    let narrowed = prefix.map(of_kind).unwrap_or_default();
    let groups = ask(
        reader,
        "actions",
        span,
        acted(narrowed),
        &table,
        &group_by,
        Some(limit),
    )
    .await?;

    let first = |answer: &Aggregate| {
        answer
            .rows
            .first()
            .map(|row| row.values.clone())
            .unwrap_or_default()
    };

    let table = groups
        .rows
        .iter()
        .map(|row| (row.group.clone(), row.values.clone()))
        .collect::<Vec<_>>();

    let mut answer = describe(&first(&said), &first(&share), &table, fields);

    if let Some(reason) = said
        .unavailable
        .or(share.unavailable)
        .or(groups.unavailable)
    {
        answer["unavailable"] = json!(reason);
    }

    Ok(answer)
}

/// What the three reads come to, as the tool says it: the first row of the statement and of the
/// share, and the rows of the table.
fn describe(
    said: &[Value],
    share: &[Value],
    groups: &[(Vec<Option<String>>, Vec<Value>)],
    fields: &[&str],
) -> Value {
    let kinds = KINDS
        .iter()
        .enumerate()
        .filter_map(|(index, (kind, _))| {
            let actions = number(said.get(1 + 2 * index));

            (actions > 0).then(|| {
                json!({
                    "kind": kind,
                    "actions": actions,
                    "sessions": number(said.get(2 + 2 * index)),
                })
            })
        })
        .collect::<Vec<_>>();

    let (sessions, affected) = (number(share.first()), number(share.get(1)));

    let rows = groups
        .iter()
        .map(|(group, values)| {
            json!({
                "values": group,
                "rage": number(values.first()),
                "dead": number(values.get(1)),
                "error": number(values.get(2)),
                "sessions": number(values.get(3)),
            })
        })
        .collect::<Vec<_>>();

    json!({
        "sessions": sessions,
        "sessionsWithSigns": number(said.first()),
        "affectedSessions": affected,
        "affectedPercent": (sessions > 0)
            .then(|| ((affected.min(sessions) as f64 / sessions as f64) * 1_000.0).round() / 10.0),
        "kinds": kinds,
        "fields": fields,
        "groups": rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::rum::query::Plan;

    #[test]
    fn a_kind_is_told_by_the_start_of_the_sign() {
        assert_eq!(prefix_of("rage"), Some("rage_"));
        assert_eq!(prefix_of("error"), Some("error_"));
        assert_eq!(prefix_of("slow"), None);
        assert_eq!(of_kind("dead_")[0].op, "contains");
    }

    #[test]
    fn a_browser_names_an_element_by_its_text_and_selector_an_app_by_its_text() {
        assert_eq!(
            fields_of(RumPlatform::Browser, "element").unwrap(),
            ["action.target.name", "action.target.selector"]
        );
        assert_eq!(
            fields_of(RumPlatform::Ios, "element").unwrap(),
            ["action.target.name"]
        );
        assert_eq!(fields_of(RumPlatform::Ios, "page").unwrap(), ["view.name"]);
        assert_eq!(
            fields_of(RumPlatform::Browser, "page").unwrap(),
            ["view.url_path_group"]
        );
        assert!(fields_of(RumPlatform::Browser, "colour").is_err());
    }

    #[test]
    fn the_measures_are_planned_for_the_columns_of_the_actions() {
        let mut statement = vec![sessions_of("sessions", Vec::new())];

        for (kind, prefix) in KINDS {
            statement.push(counted(kind, of_kind(prefix)));
            statement.push(sessions_of(&format!("{kind}_sessions"), of_kind(prefix)));
        }

        let group_by = ["action.target.name".to_string()];

        let plan = Plan::new(
            crate::domain::rum::event::RumStream::Actions,
            &crate::infra::rum::query::AggregateSpec {
                measures: &statement,
                group_by: &group_by,
                bars: None,
                order_by: Some("sessions"),
                descending: Some(true),
                limit: Some(5),
            },
        );

        assert!(plan.is_ok(), "{:?}", plan.err());
    }

    #[test]
    fn the_signs_that_did_not_occur_are_left_out_and_the_share_is_of_the_sessions() {
        let said = [
            json!(40),
            json!(120),
            json!(30),
            json!(0),
            json!(0),
            json!(15),
            json!(12),
        ];
        let share = [json!(400), json!(40)];
        let groups = vec![(
            vec![Some("Buy now".to_string()), Some("button#buy".to_string())],
            vec![json!(100), json!(0), json!(8), json!(25)],
        )];

        let described = describe(
            &said,
            &share,
            &groups,
            &["action.target.name", "action.target.selector"],
        );

        assert_eq!(described["sessionsWithSigns"], 40);
        assert_eq!(described["affectedPercent"], 10.0);
        assert_eq!(described["kinds"].as_array().unwrap().len(), 2);
        assert_eq!(described["kinds"][0]["kind"], "rage");
        assert_eq!(described["kinds"][0]["sessions"], 30);
        assert_eq!(described["kinds"][1]["kind"], "error");
        assert_eq!(described["groups"][0]["values"][1], "button#buy");
        assert_eq!(described["groups"][0]["sessions"], 25);
    }

    #[test]
    fn nothing_read_says_no_signs_and_no_share() {
        let described = describe(&[], &[], &[], &["action.target.name"]);

        assert_eq!(described["sessions"], 0);
        assert_eq!(described["affected_percent"], Value::Null);
        assert!(described["kinds"].as_array().unwrap().is_empty());
    }
}
