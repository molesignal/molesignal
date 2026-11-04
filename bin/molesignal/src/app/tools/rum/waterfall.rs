// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The loading of one page view as a waterfall: the moments the view says it had, what the largest
//! paint waited for, the resources it asked for in the order they began, and the long tasks that
//! held it up. Every time is milliseconds from the start of the view. It is read as the waterfall
//! of the performance page reads it.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::ToolResult;

use super::{Read, list_rows, summary_span};
use crate::{
    app::{
        iam::IamContext,
        tools::{
            ToolRuntime,
            common::{TimeRangeArg, parse_args},
        },
    },
    domain::rum::event::RumStream,
    infra::rum::query::{Condition, requested_columns},
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

const NANOSECONDS_PER_MILLISECOND: f64 = 1_000_000.0;

/// A request this slow or slower is called slow.
const SLOW_MS: f64 = 500.0;

/// The most resources and long tasks of one view that are read.
const RESOURCE_READ: usize = 500;
const TASK_READ: usize = 200;

const DEFAULT_LIMIT: usize = 60;
const MAX_LIMIT: usize = 200;

/// The requests and long tasks of a view are not dated to the millisecond: they are read from a
/// little before it began to a little after it was loaded.
const PADDING_MICROS: i64 = 2_000_000;

/// Longest a view is read for when it says no moment of its own.
const UNKNOWN_LOAD_CAP_MS: f64 = 15_000.0;

/// How long a URL is the model is given.
const URL_CHARS: usize = 300;

const MARKS: [(&str, &str); 5] = [
    ("first_byte", "view.first_byte"),
    ("fcp", "view.performance.fcp.timestamp"),
    ("dcl", "view.dom_content_loaded"),
    ("lcp", "view.performance.lcp.timestamp"),
    ("load", "view.load_event"),
];

const VIEW_COLUMNS: [&str; 17] = [
    "view.id",
    "view.url_host",
    "view.url_path",
    "view.loading_time",
    "view.first_byte",
    "view.performance.fcp.timestamp",
    "view.dom_content_loaded",
    "view.performance.lcp.timestamp",
    "view.performance.lcp.target_selector",
    "view.performance.lcp.resource_url",
    "view.performance.lcp.sub_parts.load_delay",
    "view.performance.lcp.sub_parts.load_time",
    "view.performance.lcp.sub_parts.render_delay",
    "view.load_event",
    "view.time_spent",
    "view.resource.count",
    "view.long_task.count",
];

const RESOURCE_COLUMNS: [&str; 21] = [
    "resource.id",
    "resource.type",
    "resource.method",
    "resource.url",
    "resource.url_host",
    "resource.url_path",
    "resource.status_code",
    "resource.duration",
    "resource.size",
    "resource.transfer_size",
    "resource.render_blocking_status",
    "resource.protocol",
    "resource.delivery_type",
    "resource.redirect.duration",
    "resource.dns.duration",
    "resource.connect.duration",
    "resource.ssl.duration",
    "resource.first_byte.duration",
    "resource.download.duration",
    "resource.trace_id",
    "resource.span_id",
];

const TASK_COLUMNS: [&str; 7] = [
    "long_task.id",
    "long_task.duration",
    "long_task.blocking_duration",
    "long_task.entry_type",
    "long_task.top_script.source_url",
    "long_task.top_script.function",
    "long_task.top_script.invoker_type",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Args {
    view_id: String,
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    only: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

/// Which resources are listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Only {
    All,
    Blocking,
    Slow,
    Failed,
}

impl Only {
    fn parse(text: Option<&str>) -> Result<Self> {
        Ok(match text.map(str::trim).filter(|text| !text.is_empty()) {
            None | Some("all") => Self::All,
            Some("blocking") => Self::Blocking,
            Some("slow") => Self::Slow,
            Some("failed") => Self::Failed,
            Some(_) => {
                return Err(Error::invalid("only must be all, blocking, slow or failed"));
            }
        })
    }

    fn admits(self, resource: &Resource) -> bool {
        match self {
            Self::All => true,
            Self::Blocking => resource.blocking,
            Self::Slow => resource.slow,
            Self::Failed => resource.failed,
        }
    }
}

/// A time the SDK sends in nanoseconds, in milliseconds.
fn millis(item: &Value, key: &str) -> Option<f64> {
    item.get(key)
        .and_then(Value::as_f64)
        .map(|nanoseconds| nanoseconds / NANOSECONDS_PER_MILLISECOND)
}

fn text(item: &Value, key: &str) -> Option<String> {
    item.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn whole(item: &Value, key: &str) -> Option<i64> {
    item.get(key).and_then(Value::as_i64)
}

/// A moment of the view rounded to a tenth of a millisecond.
fn tenth(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// A frame for a load that ends at `end`: whole steps, at most five of them.
fn axis_max(end: f64) -> f64 {
    const STEPS: [f64; 13] = [
        50.0, 100.0, 200.0, 250.0, 500.0, 1_000.0, 2_000.0, 2_500.0, 5_000.0, 10_000.0, 15_000.0,
        30_000.0, 60_000.0,
    ];

    let target = (end * 1.05).max(100.0);
    let step = STEPS
        .iter()
        .copied()
        .find(|candidate| target / candidate <= 5.0)
        .unwrap_or(120_000.0);

    (target / step).ceil() * step
}

/// What the view says of its own loading.
struct View {
    id: String,
    started_at: i64,
    host: Option<String>,
    path: Option<String>,
    marks: Vec<(&'static str, f64)>,
    loading: Option<f64>,
    time_spent: Option<f64>,
    lcp_element: Option<String>,
    lcp_resource: Option<String>,
    lcp_parts: Option<(f64, f64, f64)>,
    resources: Option<i64>,
    long_tasks: Option<i64>,
}

impl View {
    fn of(item: &Value) -> Self {
        let mut marks = MARKS
            .iter()
            .filter_map(|(key, field)| {
                millis(item, field)
                    .filter(|at| *at >= 0.0)
                    .map(|at| (*key, at))
            })
            .collect::<Vec<_>>();

        marks.sort_by(|left, right| left.1.total_cmp(&right.1));

        let part = |key: &str| millis(item, key);

        Self {
            id: text(item, "view.id").unwrap_or_default(),
            started_at: whole(item, "timestamp_micros").unwrap_or_default(),
            host: text(item, "view.url_host"),
            path: text(item, "view.url_path"),
            marks,
            loading: millis(item, "view.loading_time"),
            time_spent: millis(item, "view.time_spent"),
            lcp_element: text(item, "view.performance.lcp.target_selector"),
            lcp_resource: text(item, "view.performance.lcp.resource_url"),
            lcp_parts: part("view.performance.lcp.sub_parts.load_delay")
                .zip(part("view.performance.lcp.sub_parts.load_time"))
                .zip(part("view.performance.lcp.sub_parts.render_delay"))
                .map(|((delay, load), render)| (delay, load, render)),
            resources: whole(item, "view.resource.count"),
            long_tasks: whole(item, "view.long_task.count"),
        }
    }

    /// How long after it began the view's requests are read: until the load had ended, else a
    /// quarter of a minute.
    fn read_until(&self) -> f64 {
        let end = self
            .marks
            .iter()
            .map(|(_, at)| *at)
            .chain(self.loading)
            .fold(None, |latest: Option<f64>, at| {
                Some(latest.map_or(at, |latest| latest.max(at)))
            });

        end.map_or(UNKNOWN_LOAD_CAP_MS, axis_max)
    }

    fn describe(&self) -> Value {
        json!({
            "id": self.id,
            "startedAt": self.started_at,
            "host": self.host,
            "path": self.path,
            "loadingMs": self.loading.map(tenth),
            "timeSpentMs": self.time_spent.map(tenth),
            "moments": self.marks.iter().map(|(key, at)| json!({ "moment": key, "atMs": tenth(*at) })).collect::<Vec<_>>(),
            "largestPaint": {
                "element": self.lcp_element,
                "resource": self.lcp_resource,
                "loadDelayMs": self.lcp_parts.map(|parts| tenth(parts.0)),
                "loadTimeMs": self.lcp_parts.map(|parts| tenth(parts.1)),
                "renderDelayMs": self.lcp_parts.map(|parts| tenth(parts.2)),
            },
            "resourceCount": self.resources,
            "longTaskCount": self.long_tasks,
        })
    }
}

fn without_hash(url: &str) -> &str {
    url.split('#').next().unwrap_or(url)
}

/// A request that got no answer, or got one that says it failed.
fn has_failed(status: Option<i64>) -> bool {
    status.is_some_and(|status| status == 0 || status >= 400)
}

struct Resource {
    start: f64,
    duration: f64,
    blocking: bool,
    slow: bool,
    failed: bool,
    kind: String,
    json: Value,
}

fn resource_of(item: &Value, view: &View) -> Option<Resource> {
    let url = text(item, "resource.url")?;
    let duration = millis(item, "resource.duration")?;
    let status = whole(item, "resource.status_code");
    let started = whole(item, "timestamp_micros")?;
    let start = ((started - view.started_at) as f64 / 1_000.0).max(0.0);

    let connect = millis(item, "resource.connect.duration");
    let tls = millis(item, "resource.ssl.duration");

    // The handshake is part of the connection in the timing the browser reports: told apart, not twice.
    let phases = json!({
        "redirectMs": millis(item, "resource.redirect.duration").map(tenth),
        "dnsMs": millis(item, "resource.dns.duration").map(tenth),
        "connectMs": connect.map(|connect| tenth((connect - tls.unwrap_or(0.0)).max(0.0))),
        "tlsMs": tls.map(tenth),
        "firstByteMs": millis(item, "resource.first_byte.duration").map(tenth),
        "downloadMs": millis(item, "resource.download.duration").map(tenth),
    });

    let blocking = text(item, "resource.render_blocking_status").as_deref() == Some("blocking");
    let failed = has_failed(status);
    let slow = duration >= SLOW_MS;
    let kind = text(item, "resource.type").unwrap_or_else(|| "other".into());

    let lcp = view
        .lcp_resource
        .as_deref()
        .is_some_and(|resource| without_hash(resource) == without_hash(&url));

    let shown = url.chars().take(URL_CHARS).collect::<String>();

    Some(Resource {
        start,
        duration,
        blocking,
        slow,
        failed,
        kind: kind.clone(),
        json: json!({
            "startMs": tenth(start),
            "durationMs": tenth(duration),
            "type": kind,
            "method": text(item, "resource.method"),
            "url": shown,
            "status": status,
            "size": whole(item, "resource.size"),
            "transferSize": whole(item, "resource.transfer_size"),
            "protocol": text(item, "resource.protocol"),
            "delivery": text(item, "resource.delivery_type"),
            "renderBlocking": blocking,
            "failed": failed,
            "slow": slow,
            "largestPaint": lcp,
            "traceId": text(item, "resource.trace_id"),
            "phases": phases,
        }),
    })
}

fn task_of(item: &Value, view: &View) -> Option<(f64, Value)> {
    let duration = millis(item, "long_task.duration")?;
    let started = whole(item, "timestamp_micros")?;
    let start = ((started - view.started_at) as f64 / 1_000.0).max(0.0);

    Some((
        start,
        json!({
            "startMs": tenth(start),
            "durationMs": tenth(duration),
            "blockingMs": millis(item, "long_task.blocking_duration").map(tenth),
            "script": text(item, "long_task.top_script.source_url"),
            "function": text(item, "long_task.top_script.function"),
            "invoker": text(item, "long_task.top_script.invoker_type"),
        }),
    ))
}

/// What the resources of the view came to, whatever is listed.
fn summarize_resources(resources: &[Resource], until: f64) -> Value {
    let mut by_type: BTreeMap<&str, u64> = BTreeMap::new();

    for resource in resources {
        *by_type.entry(resource.kind.as_str()).or_default() += 1;
    }

    let mut slowest = resources.iter().collect::<Vec<_>>();

    slowest.sort_by(|left, right| right.duration.total_cmp(&left.duration));

    json!({
        "read": resources.len(),
        "blocking": resources.iter().filter(|resource| resource.blocking).count(),
        "slow": resources.iter().filter(|resource| resource.slow).count(),
        "failed": resources.iter().filter(|resource| resource.failed).count(),
        "beganAfterTheLoad": resources.iter().filter(|resource| resource.start >= until).count(),
        "byType": by_type,
        "slowest": slowest.iter().take(5).map(|resource| resource.json.clone()).collect::<Vec<_>>(),
    })
}

pub(super) async fn get(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: Args = parse_args(arguments)?;
    let only = Only::parse(args.only.as_deref())?;
    let limit = args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let id = args.view_id.trim();

    if id.is_empty() {
        return Err(Error::invalid("viewId is required"));
    }

    let span = summary_span(args.time_range)?;
    let range = TimeRange::new(TimestampMicros(span.from), TimestampMicros(span.to));
    let of_view = [Condition::equals("view.id", id)];

    let columns = requested_columns(RumStream::Views, &VIEW_COLUMNS.map(str::to_string))?;

    let Some(found) = list_rows(
        runtime,
        auth,
        &Read {
            stream: RumStream::Views,
            range,
            conditions: &of_view,
            columns: &columns,
            limit: 1,
            oldest_first: false,
        },
    )
    .await?
    else {
        return Ok(ToolResult::json(json!({
            "view": null, "streamAvailable": false,
            "message": "rum_views has not received data yet",
        })));
    };

    let Some(item) = found.items.first() else {
        return Ok(ToolResult::error(format!(
            "RUM view {id} not found in the range; give the time_range it happened in"
        )));
    };

    let view = View::of(item);
    let until = view.read_until();

    let around = TimeRange::new(
        TimestampMicros(view.started_at - PADDING_MICROS),
        TimestampMicros(view.started_at + (until * 1_000.0) as i64 + PADDING_MICROS),
    );

    let resource_columns =
        requested_columns(RumStream::Resources, &RESOURCE_COLUMNS.map(str::to_string))?;

    let task_columns = requested_columns(RumStream::LongTasks, &TASK_COLUMNS.map(str::to_string))?;

    let resources = list_rows(
        runtime,
        auth,
        &Read {
            stream: RumStream::Resources,
            range: around,
            conditions: &of_view,
            columns: &resource_columns,
            limit: RESOURCE_READ,
            oldest_first: true,
        },
    )
    .await?
    .map(|listed| listed.items)
    .unwrap_or_default();

    let tasks = list_rows(
        runtime,
        auth,
        &Read {
            stream: RumStream::LongTasks,
            range: around,
            conditions: &of_view,
            columns: &task_columns,
            limit: TASK_READ,
            oldest_first: true,
        },
    )
    .await?
    .map(|listed| listed.items)
    .unwrap_or_default();

    let mut resources = resources
        .iter()
        .filter_map(|item| resource_of(item, &view))
        .collect::<Vec<_>>();

    resources.sort_by(|left, right| left.start.total_cmp(&right.start));

    let mut tasks = tasks
        .iter()
        .filter_map(|item| task_of(item, &view))
        .collect::<Vec<_>>();

    tasks.sort_by(|left, right| left.0.total_cmp(&right.0));

    let summary = summarize_resources(&resources, until);

    let listed = resources
        .iter()
        .filter(|resource| resource.start < until && only.admits(resource))
        .collect::<Vec<_>>();

    let matching = listed.len();

    Ok(ToolResult::json(json!({
        "view": view.describe(),
        "readUntilMs": until,
        "resources": listed.iter().take(limit).map(|resource| resource.json.clone()).collect::<Vec<_>>(),
        "resourcesMatching": matching,
        "resourcesTruncated": matching > limit || resources.len() >= RESOURCE_READ,
        "resourcesSummary": summary,
        "longTasks": tasks.iter().map(|(_, task)| task.clone()).collect::<Vec<_>>(),
        "longTasksTruncated": tasks.len() >= TASK_READ,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(item: Value) -> View {
        View::of(&item)
    }

    #[test]
    fn every_column_asked_for_is_a_field_of_its_stream() {
        for (stream, columns) in [
            (RumStream::Views, VIEW_COLUMNS.to_vec()),
            (RumStream::Resources, RESOURCE_COLUMNS.to_vec()),
            (RumStream::LongTasks, TASK_COLUMNS.to_vec()),
        ] {
            let asked = columns
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>();

            assert!(
                requested_columns(stream, &asked).is_ok(),
                "{stream:?}: {:?}",
                requested_columns(stream, &asked).err()
            );
        }
    }

    #[test]
    fn the_axis_ends_in_whole_steps_after_the_load() {
        assert_eq!(axis_max(1_200.0), 1_500.0);
        assert_eq!(axis_max(10.0), 100.0);
        assert_eq!(axis_max(4_000.0), 5_000.0);
        assert_eq!(axis_max(500_000.0), 600_000.0);
    }

    #[test]
    fn a_view_is_read_until_its_load_had_ended() {
        let loaded = view(json!({
            "view.id": "v-1",
            "timestamp_micros": 1_000,
            "view.first_byte": 200_000_000,
            "view.load_event": 3_000_000_000_i64,
        }));

        assert_eq!(loaded.marks.len(), 2);
        assert_eq!(loaded.read_until(), 4_000.0);

        let silent = view(json!({ "view.id": "v-2", "timestamp_micros": 1_000 }));

        assert_eq!(silent.read_until(), UNKNOWN_LOAD_CAP_MS);
    }

    #[test]
    fn the_moments_come_in_the_order_of_time_and_a_negative_one_is_left_out() {
        let found = view(json!({
            "view.load_event": 2_000_000_000_i64,
            "view.first_byte": 100_000_000,
            "view.performance.lcp.timestamp": -1,
        }));

        assert_eq!(
            found.marks.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
            ["first_byte", "load"]
        );
    }

    #[test]
    fn a_resource_is_placed_from_the_start_of_the_view_with_its_phases() {
        let found = view(json!({
            "timestamp_micros": 10_000_000,
            "view.performance.lcp.resource_url": "https://cdn.example/hero.jpg#x",
        }));

        let resource = resource_of(
            &json!({
                "timestamp_micros": 10_250_000,
                "resource.url": "https://cdn.example/hero.jpg",
                "resource.type": "image",
                "resource.duration": 620_000_000,
                "resource.status_code": 200,
                "resource.connect.duration": 90_000_000,
                "resource.ssl.duration": 40_000_000,
                "resource.render_blocking_status": "non-blocking",
            }),
            &found,
        )
        .expect("a resource");

        assert_eq!(resource.start, 250.0);
        assert!(resource.slow && !resource.failed && !resource.blocking);
        assert_eq!(resource.json["largestPaint"], true);
        assert_eq!(resource.json["phases"]["connectMs"], 50.0);
        assert_eq!(resource.json["phases"]["tlsMs"], 40.0);
        assert_eq!(resource.json["phases"]["dnsMs"], Value::Null);
    }

    #[test]
    fn a_request_with_no_answer_or_an_error_status_has_failed() {
        assert!(has_failed(Some(0)));
        assert!(has_failed(Some(404)));
        assert!(!has_failed(Some(304)));
        assert!(!has_failed(None));
    }

    #[test]
    fn a_resource_without_a_url_or_a_duration_is_not_a_resource() {
        let found = view(json!({ "timestamp_micros": 1 }));

        assert!(
            resource_of(
                &json!({ "timestamp_micros": 1, "resource.duration": 1 }),
                &found
            )
            .is_none()
        );
        assert!(
            resource_of(
                &json!({ "timestamp_micros": 1, "resource.url": "u" }),
                &found
            )
            .is_none()
        );
    }

    #[test]
    fn what_is_listed_can_be_narrowed_to_the_blocking_the_slow_or_the_failed() {
        assert_eq!(Only::parse(None).unwrap(), Only::All);
        assert_eq!(Only::parse(Some("slow")).unwrap(), Only::Slow);
        assert!(Only::parse(Some("fast")).is_err());

        let found = view(json!({ "timestamp_micros": 0 }));

        let slow = resource_of(
            &json!({
                "timestamp_micros": 1_000,
                "resource.url": "u",
                "resource.duration": 900_000_000,
            }),
            &found,
        )
        .unwrap();

        assert!(Only::All.admits(&slow) && Only::Slow.admits(&slow));
        assert!(!Only::Failed.admits(&slow) && !Only::Blocking.admits(&slow));
    }

    #[test]
    fn a_long_task_is_placed_like_a_resource() {
        let found = view(json!({ "timestamp_micros": 5_000_000 }));

        let (start, task) = task_of(
            &json!({
                "timestamp_micros": 5_300_000,
                "long_task.duration": 120_000_000,
                "long_task.blocking_duration": 70_000_000,
                "long_task.top_script.source_url": "https://app.example/main.js",
            }),
            &found,
        )
        .expect("a task");

        assert_eq!(start, 300.0);
        assert_eq!(task["durationMs"], 120.0);
        assert_eq!(task["blockingMs"], 70.0);
        assert_eq!(task["script"], "https://app.example/main.js");
    }

    #[test]
    fn the_summary_counts_by_type_and_names_the_slowest() {
        let found = view(json!({ "timestamp_micros": 0 }));

        let resources = [
            ("a", "script", 100_000_000_i64),
            ("b", "image", 900_000_000),
            ("c", "script", 300_000_000),
        ]
        .iter()
        .filter_map(|(url, kind, duration)| {
            resource_of(
                &json!({
                    "timestamp_micros": 1_000,
                    "resource.url": url,
                    "resource.type": kind,
                    "resource.duration": duration,
                }),
                &found,
            )
        })
        .collect::<Vec<_>>();

        let summary = summarize_resources(&resources, 1_000.0);

        assert_eq!(summary["read"], 3);
        assert_eq!(summary["slow"], 1);
        assert_eq!(summary["byType"]["script"], 2);
        assert_eq!(summary["slowest"][0]["url"], "b");
    }
}
