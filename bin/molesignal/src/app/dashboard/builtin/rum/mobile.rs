// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The panels of 「RUM · 移动应用」: what a phone app has instead of the web vitals is whether it
//! crashed.

use serde_json::Value;

use super::sql::{BUCKET, FAILED, SCOPE, SESSIONS_SEEN, TOTAL, WEIGHT, p75, share};
use crate::app::dashboard::builtin::panel::{Panel, at};

pub(super) fn panels() -> Vec<Value> {
    numbers()
        .into_iter()
        .chain(trends())
        .chain(lists())
        .map(Panel::build)
        .collect()
}

fn numbers() -> Vec<Panel> {
    let sessions = |id, title, x, condition: &str, about| {
        Panel::number(
            id,
            title,
            at(x, 0, 6, 12),
            "rum_sessions",
            format!(
                r#"SELECT {} AS "{title}" FROM rum_sessions_latest WHERE {SCOPE}"#,
                share(condition)
            ),
        )
        .about(about)
        .unit("percent", 1)
    };

    vec![
        Panel::number(
            "sessions",
            "会话",
            at(0, 0, 6, 12),
            "rum_sessions",
            format!(r#"SELECT {TOTAL} AS "会话" FROM rum_sessions_latest WHERE {SCOPE}"#),
        )
        .about("时间范围内开始的会话，每个会话取最新版本。")
        .unit("short", 0),
        sessions(
            "crash-free-sessions",
            "无崩溃会话",
            6,
            r#"COALESCE("session.crash.count", 0) = 0"#,
            "没有崩溃过的会话占比。",
        ),
        sessions(
            "clean-sessions",
            "无错误会话",
            12,
            r#"COALESCE("session.error.count", 0) = 0"#,
            "没有出现过错误的会话占比。",
        ),
        sessions(
            "frustrated-sessions",
            "受挫会话",
            18,
            r#"COALESCE("session.frustration.count", 0) > 0"#,
            "出现过愤怒点击、死点击或出错点击的会话占比。",
        ),
    ]
}

fn trends() -> Vec<Panel> {
    vec![
        Panel::trend(
            "sessions-trend",
            "会话",
            at(0, 12, 12, 20),
            "rum_sessions",
            format!(
                r#"SELECT {BUCKET} AS "time", {TOTAL} AS "会话" FROM rum_sessions_latest WHERE {SCOPE} GROUP BY 1 ORDER BY 1"#
            ),
        )
        .about("每个间隔内开始的会话。")
        .unit("short", 0),
        Panel::trend(
            "crash-free-trend",
            "无崩溃会话",
            at(12, 12, 12, 20),
            "rum_sessions",
            format!(
                r#"SELECT {BUCKET} AS "time", {} AS "无崩溃会话" FROM rum_sessions_latest WHERE {SCOPE} GROUP BY 1 ORDER BY 1"#,
                share(r#"COALESCE("session.crash.count", 0) = 0"#)
            ),
        )
        .about("每个间隔内开始的会话里，没有崩溃过的占比。")
        .unit("percent", 1),
        Panel::trend(
            "errors-trend",
            "错误与崩溃",
            at(0, 32, 24, 18),
            "rum_errors",
            format!(
                r#"SELECT {BUCKET} AS "time", CAST(COALESCE(ROUND(SUM({WEIGHT}) FILTER (WHERE NOT COALESCE("error.is_crash", false))), 0) AS BIGINT) AS "错误", CAST(COALESCE(ROUND(SUM({WEIGHT}) FILTER (WHERE COALESCE("error.is_crash", false))), 0) AS BIGINT) AS "崩溃" FROM rum_errors WHERE {SCOPE} GROUP BY 1 ORDER BY 1"#
            ),
        )
        .about("每个间隔内的错误与崩溃。")
        .unit("short", 0)
        .legend(),
    ]
}

fn lists() -> Vec<Panel> {
    let crashed = share(r#"COALESCE("view.crash.count", 0) > 0"#);
    let errored = share(r#"COALESCE("view.error.count", 0) > 0"#);
    let crashed_sessions = share(r#"COALESCE("session.crash.count", 0) > 0"#);

    vec![
        Panel::table(
            "screens",
            "屏幕",
            at(0, 50, 12, 22),
            "rum_views",
            format!(
                r#"SELECT "view.name" AS "屏幕", {TOTAL} AS "视图", {errored} AS "错误率", {crashed} AS "崩溃率" FROM rum_views_latest WHERE {SCOPE} GROUP BY 1 ORDER BY 2 DESC LIMIT 6"#
            ),
        )
        .about("浏览最多的 6 个屏幕。")
        .column("视图", "short", 0)
        .column("错误率", "percent", 1)
        .column("崩溃率", "percent", 1),
        Panel::table(
            "versions",
            "版本",
            at(12, 50, 12, 22),
            "rum_sessions",
            format!(
                r#"SELECT version AS "版本", {TOTAL} AS "会话", {crashed_sessions} AS "崩溃会话" FROM rum_sessions_latest WHERE {SCOPE} AND version IS NOT NULL GROUP BY 1 ORDER BY max(_timestamp) DESC LIMIT 4"#
            ),
        )
        .about("最近出现的 4 个版本。")
        .column("会话", "short", 0)
        .column("崩溃会话", "percent", 1),
        Panel::table(
            "crashes-by-version",
            "崩溃 · 版本",
            at(0, 72, 12, 22),
            "rum_errors",
            format!(
                r#"SELECT version AS "版本", {TOTAL} AS "崩溃", {SESSIONS_SEEN} AS "会话" FROM rum_errors WHERE {SCOPE} AND COALESCE("error.is_crash", false) AND version IS NOT NULL GROUP BY 1 ORDER BY 2 DESC LIMIT 5"#
            ),
        )
        .about("崩溃最多的版本，与崩溃影响到的会话。")
        .column("崩溃", "short", 0)
        .column("会话", "short", 0),
        Panel::table(
            "frustrated-elements",
            "受挫元素",
            at(12, 72, 12, 22),
            "rum_actions",
            format!(
                r#"SELECT COALESCE("action.target.name", "action.target.selector") AS "元素", "view.name" AS "屏幕", {SESSIONS_SEEN} AS "会话" FROM rum_actions WHERE {SCOPE} AND COALESCE("action.frustration.count", 0) > 0 GROUP BY 1, 2 ORDER BY 3 DESC LIMIT 5"#
            ),
        )
        .about("出现受挫点击的会话最多的元素。")
        .column("会话", "short", 0),
        Panel::table(
            "slow-hosts",
            "慢资源主机",
            at(0, 94, 12, 22),
            "rum_resources",
            format!(
                r#"SELECT "resource.url_host" AS "主机", {} AS "p75 耗时", {} AS "失败率" FROM rum_resources WHERE {SCOPE} AND "resource.type" IN ('fetch', 'xhr') GROUP BY 1 HAVING count(*) >= 10 ORDER BY 2 DESC LIMIT 5"#,
                p75("resource.duration", "1e6"),
                share(FAILED)
            ),
        )
        .about("接口请求的 p75 耗时最长的主机；失败是没有应答或服务端出错的请求占比。")
        .column("p75 耗时", "milliseconds", 0)
        .column("失败率", "percent", 1),
        Panel::bars(
            "devices",
            "设备",
            at(18, 94, 6, 22),
            "rum_sessions",
            format!(
                r#"SELECT COALESCE("device.model", "device.type") AS "设备", {TOTAL} AS "会话" FROM rum_sessions_latest WHERE {SCOPE} AND COALESCE("device.model", "device.type") IS NOT NULL GROUP BY 1 ORDER BY 2 DESC LIMIT 5"#
            ),
        )
        .about("会话最多的 5 种设备。")
        .unit("short", 0),
        Panel::bars(
            "systems",
            "系统",
            at(12, 94, 6, 22),
            "rum_sessions",
            format!(
                r#"SELECT "os.name" AS "系统", {TOTAL} AS "会话" FROM rum_sessions_latest WHERE {SCOPE} AND "os.name" IS NOT NULL GROUP BY 1 ORDER BY 2 DESC LIMIT 5"#
            ),
        )
        .about("会话最多的 5 种操作系统。")
        .unit("short", 0),
    ]
}
