// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output, time_range_schema};
use crate::ToolSpec;

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let common = json!({
        "timeRange": time_range_schema(), "application": {"type": "string"},
        "environment": {"type": "string"},
        "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}
    });

    let (description, input, tags) = match kind {
        BuiltinToolKind::ListRumSessions => {
            let mut fields = common.clone();
            fields["endUserId"] = json!({"type": "string"});
            fields["sessionType"] = json!({
                "type": "string",
                "enum": ["user", "synthetics", "ci_test"],
                "description": "Which kind of session to list; the sessions people ran unless it says"
            });

            (
                "List bounded real-user monitoring sessions.",
                object_schema(fields),
                vec!["RUM", "Sessions"],
            )
        }
        BuiltinToolKind::GetRumSession => (
            "Get one RUM session with its views, actions, errors and browser logs in the order they happened, each list bounded.",
            json!({"type": "object", "required": ["sessionId"], "properties": {"sessionId": {"type": "string"}, "timeRange": time_range_schema(), "eventLimit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}}, "additionalProperties": false}),
            vec!["RUM", "Sessions"],
        ),
        BuiltinToolKind::ListRumActions => {
            let mut fields = common.clone();
            fields["sessionId"] = json!({"type": "string"});
            fields["actionType"] =
                json!({"type": "string", "description": "Only with the default `actions` stream"});
            fields["stream"] = json!({
                "type": "string",
                "enum": ["actions", "views", "resources", "long_tasks", "vitals", "logs"],
                "default": "actions",
                "description": "Which rows to list: user actions, page views, resource requests, long tasks, vitals or the logs of the browser"
            });
            fields["sessionType"] = json!({
                "type": "string",
                "enum": ["user", "synthetics", "ci_test"],
                "description": "Which kind of session to list; the sessions people ran unless it says, or a session is named"
            });

            (
                "List what real users did, newest first: user actions, or with `stream` their page views, resource requests, long tasks, vitals and browser logs.",
                object_schema(fields),
                vec!["RUM", "Actions"],
            )
        }
        BuiltinToolKind::ListRumErrors => {
            let mut fields = common;
            fields["sessionId"] = json!({"type": "string"});
            fields["fingerprint"] = json!({"type": "string"});

            (
                "List RUM errors by session or fingerprint.",
                object_schema(fields),
                vec!["RUM", "Errors"],
            )
        }
        BuiltinToolKind::GetRumRelatedTraces => (
            "Find traces directly linked to a RUM session, with bounded time-correlation fallback.",
            json!({"type": "object", "required": ["sessionId"], "properties": {"sessionId": {"type": "string"}, "timeRange": time_range_schema(), "limit": {"type": "integer", "minimum": 1, "maximum": 100, "default": 20}}, "additionalProperties": false}),
            vec!["RUM", "Traces", "Correlation"],
        ),
        BuiltinToolKind::GetRumVitals => {
            let mut fields = common.clone();
            fields["page"] =
                json!({"type": "string", "description": "Only pages whose path contains this"});
            fields["limit"] =
                json!({"type": "integer", "minimum": 1, "maximum": 200, "default": 50});

            (
                "Summarize the Core Web Vitals real users had, page by page: the 75th percentile of LCP, INP, CLS and TTFB with the number of page views, slowest LCP first, and the thresholds a good and a poor value cross.",
                object_schema(fields),
                vec!["RUM", "Web Vitals", "Performance"],
            )
        }
        BuiltinToolKind::ListRumIssues => (
            "List the error issues of a RUM application. Errors that are alike are one issue with a status, the times and versions it was first and last seen in and, over the range, the sessions, users and occurrences it touched with a trend. Ranked by the sessions touched unless `sort` says otherwise.",
            json!({
                "type": "object",
                "required": ["application"],
                "properties": {
                    "application": {"type": "string", "description": "RUM application id"},
                    "environment": {"type": "string"},
                    "timeRange": time_range_schema(),
                    "status": {
                        "type": "string",
                        "enum": ["unresolved", "all", "open", "investigating", "regressed", "resolved", "ignored"],
                        "default": "unresolved"
                    },
                    "sort": {
                        "type": "string",
                        "enum": ["sessions", "users", "occurrences", "last_seen", "first_seen", "status"],
                        "default": "sessions"
                    },
                    "search": {"type": "string", "description": "Words the type, message or place of an issue has"},
                    "fresh": {"type": "boolean", "description": "Only the issues first seen in the range or regressed in it"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 50, "default": 20}
                },
                "additionalProperties": false
            }),
            vec!["RUM", "Errors", "Issues"],
        ),
        BuiltinToolKind::GetRumIssue => (
            "Get one RUM issue: what it is, its status and history (first and last seen, versions, regression), what it came to in the range, the latest sessions it happened in and the stack of the latest error that has one.",
            json!({
                "type": "object",
                "required": ["issueId"],
                "properties": {
                    "issueId": {"type": "string"},
                    "timeRange": time_range_schema()
                },
                "additionalProperties": false
            }),
            vec!["RUM", "Errors", "Issues"],
        ),
        BuiltinToolKind::GetRumOverview => (
            "Summarize how a RUM application did over a range (the last 24 hours by default): sessions and people, the sessions without an error or a crash, frustrated sessions, the Core Web Vitals of a browser application (75th percentile and rating), the most viewed pages and the unresolved issues touching the most sessions. Start here.",
            json!({
                "type": "object",
                "required": ["application"],
                "properties": {
                    "application": {"type": "string", "description": "RUM application id"},
                    "environment": {"type": "string"},
                    "timeRange": time_range_schema()
                },
                "additionalProperties": false
            }),
            vec!["RUM", "Overview"],
        ),
        BuiltinToolKind::GetRumFrustration => (
            "Say what frustrates the users of a RUM application over a range (the last 24 hours by default): how many actions showed rage, dead or error clicks (taps in an app) and in how many sessions, and which elements, pages, versions, browsers or systems they concentrate in.",
            json!({
                "type": "object",
                "required": ["application"],
                "properties": {
                    "application": {"type": "string", "description": "RUM application id"},
                    "environment": {"type": "string"},
                    "timeRange": time_range_schema(),
                    "kind": {"type": "string", "enum": ["rage", "dead", "error"], "description": "Only this sign of frustration"},
                    "groupBy": {
                        "type": "string",
                        "enum": ["element", "page", "version", "browser", "system"],
                        "default": "element"
                    },
                    "limit": {"type": "integer", "minimum": 1, "maximum": 50, "default": 20}
                },
                "additionalProperties": false
            }),
            vec!["RUM", "Frustration"],
        ),
        BuiltinToolKind::GetRumViewWaterfall => (
            "Get the loading of one page view as a waterfall: its moments (first byte, first and largest paint, DOM ready, load), what the largest paint waited for, the resources it requested in the order they began with their phases, and the long tasks that blocked it. Times are milliseconds from the start of the view.",
            json!({
                "type": "object",
                "required": ["viewId"],
                "properties": {
                    "viewId": {"type": "string"},
                    "timeRange": time_range_schema(),
                    "only": {
                        "type": "string",
                        "enum": ["all", "blocking", "slow", "failed"],
                        "default": "all",
                        "description": "Which resources to list: all of them, the render-blocking ones, those that took 500 ms or more, or those that failed"
                    },
                    "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 60}
                },
                "additionalProperties": false
            }),
            vec!["RUM", "Performance", "Waterfall"],
        ),
        BuiltinToolKind::AggregateRum => (
            "Count, sum, average and take percentiles over every row of one RUM stream that matches, for each group of up to two fields and optionally for each bar of time, as the explorer's charts and tables do. Fields are the dotted column names of the stream; durations are nanoseconds. A filter is {field, op, value} with op one of =, !=, in, contains, >, >=, <, <=, exists, missing. An application may keep only a share of its sessions: counts, sums and averages are then estimates of what there would be had every session been kept (each row counts for as many as the share it was kept with says; errors are all kept, and the people seen are not scaled), unless `estimate` is false, which counts the rows that are stored.",
            json!({
                "type": "object",
                "required": ["type"],
                "properties": {
                    "type": {
                        "type": "string",
                        "enum": ["sessions", "views", "actions", "errors", "resources", "long_tasks", "vitals"]
                    },
                    "application": {"type": "string"},
                    "environment": {"type": "string"},
                    "timeRange": time_range_schema(),
                    "filters": {
                        "type": "array",
                        "maxItems": 16,
                        "items": {
                            "type": "object",
                            "required": ["field", "op"],
                            "properties": {
                                "field": {"type": "string"},
                                "op": {"type": "string"},
                                "value": {"type": "string"}
                            },
                            "additionalProperties": false
                        }
                    },
                    "freeText": {"type": "array", "maxItems": 4, "items": {"type": "string"}},
                    "measures": {
                        "type": "array",
                        "maxItems": 8,
                        "description": "What to compute; one count of the rows when none is given",
                        "items": {
                            "type": "object",
                            "required": ["fn"],
                            "properties": {
                                "fn": {
                                    "type": "string",
                                    "enum": ["count", "count_distinct", "sum", "avg", "min", "max", "p50", "p75", "p90", "p95", "p99"]
                                },
                                "field": {"type": "string"},
                                "fallback": {"type": "string", "description": "For count_distinct: the field to take where `field` has no value"},
                                "name": {"type": "string"},
                                "where": {"type": "array", "maxItems": 8, "items": {"type": "object"}}
                            },
                            "additionalProperties": false
                        }
                    },
                    "groupBy": {"type": "array", "maxItems": 2, "items": {"type": "string"}},
                    "bucketSeconds": {"type": "integer", "minimum": 1, "description": "Also split the answer into bars of this many seconds"},
                    "orderBy": {"type": "string", "description": "The name of a measure, or a group field"},
                    "order": {"type": "string", "enum": ["asc", "desc"]},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100, "default": 20},
                    "estimate": {"type": "boolean", "default": true, "description": "Count what the rows kept stand for (default), or the rows that are stored"}
                },
                "additionalProperties": false
            }),
            vec!["RUM", "Aggregate", "Analytics"],
        ),
        BuiltinToolKind::GetRumFunnel => (
            "Count how many sessions of a RUM application got through a series of steps in order (the last 24 hours unless told otherwise), as the funnel of the analytics tab does: for each step the sessions that did it after the one before, the share of the first step's and of the step before's, the median time between steps, and optionally the same by a field of the first step's rows such as the browser or the version. A step is a page or screen that was opened (`views`) or something a person did (`actions`).",
            json!({
                "type": "object",
                "required": ["application", "steps"],
                "properties": {
                    "application": {"type": "string", "description": "RUM application id"},
                    "environment": {"type": "string"},
                    "timeRange": time_range_schema(),
                    "steps": {
                        "type": "array",
                        "minItems": 2,
                        "maxItems": 8,
                        "items": {
                            "type": "object",
                            "required": ["type", "value"],
                            "properties": {
                                "type": {"type": "string", "enum": ["views", "actions"]},
                                "field": {
                                    "type": "string",
                                    "description": "The field the value is looked for in. By default the path of the page with its changing parts grouped (`view.url_path_group`, `view.name` for an app) for views, and the name of the element (`action.target.name`) for actions; others are `view.url_path`, `view.name`, `action.target.selector`, `action.type`"
                                },
                                "op": {"type": "string", "enum": ["=", "contains"], "default": "="},
                                "value": {"type": "string", "description": "What to look for, such as `/checkout` or `Pay now`"},
                                "name": {"type": "string", "description": "What the step is called in the answer; the value by default"}
                            },
                            "additionalProperties": false
                        }
                    },
                    "windowSeconds": {"type": "integer", "minimum": 1, "description": "The longest from the first step to a later one; as long as it takes by default"},
                    "breakdown": {"type": "string", "description": "A text field of the first step's rows to count the sessions by, such as `browser.name`, `os.name`, `device.type`, `version` or `geo.country_iso_code`; the 10 values with the most sessions are kept"}
                },
                "additionalProperties": false
            }),
            vec!["RUM", "Funnel", "Analytics"],
        ),
        BuiltinToolKind::GetRumPaths => (
            "Follow the pages sessions of a RUM application opened after a page, or before it (the last 24 hours unless told otherwise), as the paths of the analytics tab do: the most visited pages at each step of the way with the sessions that were there and how many went no further, and the sessions that went from one page to the next. Without `page` the paths start at the first page of each session, or end at the last one going back.",
            json!({
                "type": "object",
                "required": ["application"],
                "properties": {
                    "application": {"type": "string", "description": "RUM application id"},
                    "environment": {"type": "string"},
                    "timeRange": time_range_schema(),
                    "page": {"type": "string", "description": "The page the paths start from (end at when `direction` is `before`), told by `page_field`"},
                    "direction": {"type": "string", "enum": ["after", "before"], "default": "after"},
                    "depth": {"type": "integer", "minimum": 1, "maximum": 6, "default": 3, "description": "How many pages after (before) the page"},
                    "pageField": {
                        "type": "string",
                        "enum": ["view.url_path_group", "view.url_path", "view.name"],
                        "description": "How a page is told: by the path with its changing parts grouped, the exact path or the name of the view. `view.name` for an app"
                    }
                },
                "additionalProperties": false
            }),
            vec!["RUM", "Paths", "Analytics"],
        ),
        BuiltinToolKind::GetRumRetention => (
            "Say how many of the people of a RUM application who were first seen in a day or a week were seen again in each of the periods after, as the retention of the analytics tab does (UTC days and Monday weeks; a person is their user id, or the anonymous id of the visitor). A period that has not ended by the end of the range has no count. Looks back far enough to have cohorts unless a range is given.",
            json!({
                "type": "object",
                "required": ["application"],
                "properties": {
                    "application": {"type": "string", "description": "RUM application id"},
                    "environment": {"type": "string"},
                    "timeRange": time_range_schema(),
                    "period": {"type": "string", "enum": ["day", "week"], "default": "day"},
                    "periods": {"type": "integer", "minimum": 1, "maximum": 30, "default": 7, "description": "How many periods after the first to read"}
                },
                "additionalProperties": false
            }),
            vec!["RUM", "Retention", "Analytics"],
        ),
        BuiltinToolKind::GetRumFeatureFlags => (
            "Say which feature flags the sessions of a RUM application evaluated over a range (the last 24 hours by default), most sessions first; with `flag`, compare the sessions of each of its values: sessions, views, views that met an error or frustration, and the 75th percentile of LCP, INP and CLS. Any other field of a flag can be read with `aggregate_rum`, as `feature_flags.<flag>`.",
            json!({
                "type": "object",
                "required": ["application"],
                "properties": {
                    "application": {"type": "string", "description": "RUM application id"},
                    "environment": {"type": "string"},
                    "timeRange": time_range_schema(),
                    "flag": {"type": "string", "description": "One flag, to compare its values"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100, "default": 20}
                },
                "additionalProperties": false
            }),
            vec!["RUM", "Feature flags"],
        ),
        BuiltinToolKind::GetRumHeatmap => (
            "Say where people click on one page of a browser application and how far down they go (the last 24 hours by default): the elements with the most clicks, with the sessions and the frustrated clicks of each, how long the page is, and the share of views that reached each depth.",
            json!({
                "type": "object",
                "required": ["application", "page"],
                "properties": {
                    "application": {"type": "string", "description": "RUM application id"},
                    "page": {"type": "string", "description": "The path of the page with its changing parts grouped (`view.url_path_group`), such as `/products/?`"},
                    "environment": {"type": "string"},
                    "timeRange": time_range_schema(),
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100, "default": 20}
                },
                "additionalProperties": false
            }),
            vec!["RUM", "Heatmap", "Analytics"],
        ),
        BuiltinToolKind::GetRumSessionNotes => (
            "Read what is kept about a recorded RUM session besides its events: what people said about moments of it, and the summary and chapters a model wrote of it, if anyone asked for one.",
            json!({
                "type": "object",
                "required": ["sessionId"],
                "properties": {"sessionId": {"type": "string"}},
                "additionalProperties": false
            }),
            vec!["RUM", "Session replay"],
        ),
        _ => unreachable!("RUM catalog received unrelated kind"),
    };

    ToolSpec::read(
        kind.name(),
        description,
        "observability",
        "rum",
        input,
        open_output(),
        &["rum.events.read", "sys.telemetry.read"],
        &tags,
    )
    .any_permission()
}
