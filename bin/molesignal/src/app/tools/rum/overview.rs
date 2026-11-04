// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How a RUM application did over a range, in one tool: what the overview page opens with, and the
//! unresolved issues touching the most sessions, so the next question has somewhere to start.

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::ToolResult;

use super::{reader, summary_span};
use crate::{
    app::{
        iam::IamContext,
        rum::{
            issues::{
                self, Query,
                select::{Holder, Sort, StatusFilter},
            },
            overview::{self, Overview},
        },
        tools::{
            ToolRuntime,
            common::{TimeRangeArg, parse_args},
        },
    },
    domain::rum::RumApplication,
    shared::{Error, Result},
};

/// How many issues the overview names.
const ISSUES: usize = 3;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Args {
    application: String,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
}

pub(super) async fn summarize(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: Args = parse_args(arguments)?;
    let span = summary_span(args.time_range)?;
    let id = args.application.trim();

    let application = match runtime
        .observability
        .rum_applications
        .get(&auth.org_id, id)
        .await
    {
        Ok(application) => application,
        Err(Error::NotFound(_)) => {
            return Ok(ToolResult::error(format!("RUM application {id} not found")));
        }
        Err(error) => return Err(error),
    };

    let environment = args
        .environment
        .as_deref()
        .map(str::trim)
        .filter(|environment| !environment.is_empty());

    let reader = reader(runtime, auth);

    let numbers = overview::read(&reader, &application, environment, span).await?;

    let query = Query {
        application: application.id.clone(),
        env: environment.map(str::to_string),
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
        limit: ISSUES,
    };

    let top = issues::list(&reader, runtime.observability.rum_issues.as_ref(), &query).await?;

    let mut answer = describe(&application, environment, &numbers);

    answer["range"] = json!({ "startTime": span.from, "endTime": span.to });
    answer["topIssues"] = serde_json::to_value(&top.issues)
        .map_err(|error| Error::internal(format!("serialize tool result: {error}")))?;
    answer["unresolvedIssues"] = json!(top.total);

    if let Some(reason) = top.unavailable
        && answer.get("unavailable").is_none()
    {
        answer["unavailable"] = json!(reason);
    }

    Ok(ToolResult::json(answer))
}

/// A share of the sessions as a percentage with one decimal, as the page says it.
fn percent(share: Option<f64>) -> Value {
    share.map_or(Value::Null, |share| json!((share * 1_000.0).round() / 10.0))
}

/// What the overview comes to, as the tool says it.
fn describe(application: &RumApplication, environment: Option<&str>, numbers: &Overview) -> Value {
    let pulse = &numbers.pulse;

    let mut answer = json!({
        "application": {
            "id": application.id,
            "name": application.name,
            "platform": application.platform,
        },
        "environment": environment,
        "sessions": {
            "total": pulse.sessions,
            "stored": pulse.stored_sessions,
            "users": pulse.users,
            "withErrors": pulse.with_errors,
            "withCrashes": pulse.with_crashes,
            "usersWithCrashes": pulse.users_with_crashes,
            "frustrated": pulse.frustrated,
            "withoutErrorsPercent": percent(pulse.error_free_share()),
            "withoutCrashesPercent": percent(pulse.crash_free_share()),
        },
        "views": numbers.views,
        "vitals": numbers.vitals,
        "topPages": numbers.pages,
        "vitalsNote": "75th percentile over every view; lcp_ms and inp_ms are milliseconds, cls is a score",
    });

    // An application that keeps a share of its sessions has them counted for as many as the share
    // says; the people seen are as they were seen.
    if pulse.stored_sessions < pulse.sessions {
        answer["estimateNote"] = json!(
            "only some of the sessions are stored: the counts are estimates scaled up from the stored sessions (`stored`), and the people seen (`users`) are not scaled"
        );
    }

    if let Some(reason) = numbers.unavailable {
        answer["unavailable"] = json!(reason);
    }

    answer
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::rum::{overview::Pulse, vitals::Reading},
        domain::rum::{RumApplicationSettings, RumPlatform},
        shared::{ids::Id, time::TimestampMicros},
    };

    fn application() -> RumApplication {
        RumApplication {
            org_id: Id::from_string("org"),
            id: "shop-web".into(),
            name: "Shop".into(),
            platform: RumPlatform::Browser,
            settings: RumApplicationSettings::default(),
            created_by: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    #[test]
    fn a_share_is_a_percentage_with_one_decimal() {
        assert_eq!(percent(Some(0.9876)), json!(98.8));
        assert_eq!(percent(Some(1.0)), json!(100.0));
        assert_eq!(percent(None), Value::Null);
    }

    #[test]
    fn the_overview_says_the_sessions_the_vitals_and_the_pages() {
        let numbers = Overview {
            pulse: Pulse {
                sessions: 200,
                users: 150,
                with_errors: 30,
                with_crashes: 0,
                users_with_crashes: 0,
                frustrated: 12,
                stored_sessions: 50,
            },
            vitals: vec![Reading {
                name: "lcp_ms",
                p75: 2_100.0,
                samples: 180,
                rating: "good",
            }],
            views: 640,
            pages: Vec::new(),
            unavailable: None,
        };

        let answer = describe(&application(), Some("prod"), &numbers);

        assert_eq!(answer["application"]["platform"], "browser");
        assert_eq!(answer["environment"], "prod");
        assert_eq!(answer["sessions"]["total"], 200);
        assert_eq!(answer["sessions"]["stored"], 50);
        assert_eq!(answer["sessions"]["withoutErrorsPercent"], 85.0);
        assert_eq!(answer["sessions"]["withoutCrashesPercent"], 100.0);
        assert_eq!(answer["vitals"][0]["rating"], "good");
        assert!(answer.get("unavailable").is_none());

        // Only a quarter of the sessions are stored: the counts are estimates, and the answer says so.
        assert!(
            answer["estimateNote"]
                .as_str()
                .unwrap()
                .contains("estimates")
        );
    }

    #[test]
    fn nothing_is_said_of_estimates_when_every_session_is_stored() {
        let numbers = Overview {
            pulse: Pulse {
                sessions: 200,
                stored_sessions: 200,
                ..Pulse::default()
            },
            vitals: Vec::new(),
            views: 0,
            pages: Vec::new(),
            unavailable: None,
        };

        assert!(
            describe(&application(), None, &numbers)
                .get("estimateNote")
                .is_none()
        );
    }
}
