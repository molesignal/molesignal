// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The panels of 「RUM · 浏览器应用」.

use serde_json::Value;

use super::sql::{
    BUCKET, CLS, FAILED, INP, LCP, PAGE, SCOPE, SESSIONS_SEEN, TOTAL, WEIGHT, p75, share,
};
use crate::app::dashboard::builtin::panel::{GOOD, POOR, Panel, WATCH, at, line, step};

pub(super) fn panels() -> Vec<Value> {
    numbers()
        .into_iter()
        .chain(trends())
        .chain(lists())
        .map(Panel::build)
        .collect()
}

/// The first row: the figures of the overview, the vitals graded by the lines web.dev draws.
fn numbers() -> Vec<Panel> {
    let vitals = |good: f64, poor: f64| {
        [
            step(None, GOOD),
            step(Some(good), WATCH),
            step(Some(poor), POOR),
        ]
    };

    vec![
        Panel::number(
            "sessions",
            "会话",
            at(0, 0, 4, 12),
            "rum_sessions",
            format!(r#"SELECT {TOTAL} AS "会话" FROM rum_sessions_latest WHERE {SCOPE}"#),
        )
        .about("时间范围内开始的会话，每个会话取最新版本。")
        .unit("short", 0),
        Panel::number(
            "clean-sessions",
            "无错误会话",
            at(4, 0, 4, 12),
            "rum_sessions",
            format!(
                r#"SELECT {} AS "无错误会话" FROM rum_sessions_latest WHERE {SCOPE}"#,
                share(r#"COALESCE("session.error.count", 0) = 0"#)
            ),
        )
        .about("没有出现过错误的会话占比。")
        .unit("percent", 1),
        Panel::number(
            "lcp",
            "LCP p75",
            at(8, 0, 4, 12),
            "rum_views",
            format!(
                r#"SELECT {} AS "LCP p75" FROM rum_views_latest WHERE {SCOPE}"#,
                p75(LCP, "1e9")
            ),
        )
        .about("最大内容绘制的第 75 百分位，只算有这个值的页面视图。良好不超过 2.5 秒，超过 4 秒为差。")
        .unit("seconds", 2)
        .graded(&vitals(2.5, 4.0)),
        Panel::number(
            "inp",
            "INP p75",
            at(12, 0, 4, 12),
            "rum_views",
            format!(
                r#"SELECT {} AS "INP p75" FROM rum_views_latest WHERE {SCOPE}"#,
                p75(INP, "1e6")
            ),
        )
        .about("下次绘制的交互延迟的第 75 百分位，只算有这个值的页面视图。良好不超过 200 毫秒，超过 500 毫秒为差。")
        .unit("milliseconds", 0)
        .graded(&vitals(200.0, 500.0)),
        Panel::number(
            "cls",
            "CLS p75",
            at(16, 0, 4, 12),
            "rum_views",
            format!(
                r#"SELECT {} AS "CLS p75" FROM rum_views_latest WHERE {SCOPE}"#,
                p75(CLS, "1")
            ),
        )
        .about("累计布局偏移的第 75 百分位，只算有这个值的页面视图。良好不超过 0.1，超过 0.25 为差。")
        .unit("none", 2)
        .graded(&vitals(0.1, 0.25)),
        Panel::number(
            "frustrated-sessions",
            "受挫会话",
            at(20, 0, 4, 12),
            "rum_sessions",
            format!(
                r#"SELECT {} AS "受挫会话" FROM rum_sessions_latest WHERE {SCOPE}"#,
                share(r#"COALESCE("session.frustration.count", 0) > 0"#)
            ),
        )
        .about("出现过愤怒点击、死点击或出错点击的会话占比。")
        .unit("percent", 1),
    ]
}

/// Two charts of what happens, then the three vitals over time with the lines between good and
/// poor.
fn trends() -> Vec<Panel> {
    let vital = |id, title, x, column: &str, per: &str, unit, decimals, good, poor| {
        Panel::trend(
            id,
            title,
            at(x, 32, 8, 18),
            "rum_views",
            format!(
                r#"SELECT {BUCKET} AS "time", {} AS "{title}" FROM rum_views_latest WHERE {SCOPE} GROUP BY 1 ORDER BY 1"#,
                p75(column, per)
            ),
        )
        .about("每个间隔的第 75 百分位；图里的线是良好与差的界限。")
        .unit(unit, decimals)
        .graded(&[line(good, WATCH, "良好"), line(poor, POOR, "差")])
    };

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
            "errors-trend",
            "错误",
            at(12, 12, 12, 20),
            "rum_errors",
            format!(
                r#"SELECT {BUCKET} AS "time", CAST(COALESCE(ROUND(SUM({WEIGHT}) FILTER (WHERE COALESCE("error.source", '') <> 'network')), 0) AS BIGINT) AS "代码", CAST(COALESCE(ROUND(SUM({WEIGHT}) FILTER (WHERE "error.source" = 'network')), 0) AS BIGINT) AS "网络" FROM rum_errors WHERE {SCOPE} GROUP BY 1 ORDER BY 1"#
            ),
        )
        .about("每个间隔内的错误：代码里抛出的，与请求失败的。")
        .unit("short", 0)
        .legend(),
        vital("lcp-trend", "LCP p75", 0, LCP, "1e9", "seconds", 2, 2.5, 4.0),
        vital("inp-trend", "INP p75", 8, INP, "1e6", "milliseconds", 0, 200.0, 500.0),
        vital("cls-trend", "CLS p75", 16, CLS, "1", "none", 2, 0.1, 0.25),
    ]
}

/// Where the figures come from: pages, versions, the elements people got stuck on, the hosts that
/// answer slowly, and who the users are.
fn lists() -> Vec<Panel> {
    let failed_share = share(FAILED);
    let errored_share = share(r#"COALESCE("view.error.count", 0) > 0"#);

    vec![
        Panel::table(
            "pages",
            "页面",
            at(0, 50, 12, 22),
            "rum_views",
            format!(
                r#"SELECT {PAGE} AS "页面", {TOTAL} AS "视图", {} AS "LCP p75", {} AS "INP p75", {errored_share} AS "错误率" FROM rum_views_latest WHERE {SCOPE} GROUP BY 1 ORDER BY 2 DESC LIMIT 6"#,
                p75(LCP, "1e9"),
                p75(INP, "1e6")
            ),
        )
        .about("浏览最多的 6 个页面。")
        .column("视图", "short", 0)
        .column("LCP p75", "seconds", 2)
        .column("INP p75", "milliseconds", 0)
        .column("错误率", "percent", 1),
        Panel::table(
            "versions",
            "版本",
            at(12, 50, 12, 22),
            "rum_views",
            format!(
                r#"SELECT version AS "版本", {SESSIONS_SEEN} AS "会话", {} AS "LCP p75", {errored_share} AS "错误率" FROM rum_views_latest WHERE {SCOPE} AND version IS NOT NULL GROUP BY 1 ORDER BY max(_timestamp) DESC LIMIT 4"#,
                p75(LCP, "1e9")
            ),
        )
        .about("最近出现的 4 个版本。")
        .column("会话", "short", 0)
        .column("LCP p75", "seconds", 2)
        .column("错误率", "percent", 1),
        Panel::table(
            "frustrated-elements",
            "受挫元素",
            at(0, 72, 12, 22),
            "rum_actions",
            format!(
                r#"SELECT COALESCE("action.target.name", "action.target.selector") AS "元素", "view.url_path_group" AS "页面", {SESSIONS_SEEN} AS "会话" FROM rum_actions WHERE {SCOPE} AND COALESCE("action.frustration.count", 0) > 0 GROUP BY 1, 2 ORDER BY 3 DESC LIMIT 5"#
            ),
        )
        .about("出现受挫点击的会话最多的元素。")
        .column("会话", "short", 0),
        Panel::table(
            "slow-hosts",
            "慢资源主机",
            at(12, 72, 12, 22),
            "rum_resources",
            format!(
                r#"SELECT "resource.url_host" AS "主机", {} AS "p75 耗时", {failed_share} AS "失败率" FROM rum_resources WHERE {SCOPE} AND "resource.type" IN ('fetch', 'xhr') GROUP BY 1 HAVING count(*) >= 10 ORDER BY 2 DESC LIMIT 5"#,
                p75("resource.duration", "1e6")
            ),
        )
        .about("接口请求的 p75 耗时最长的主机；失败是没有应答或服务端出错的请求占比。")
        .column("p75 耗时", "milliseconds", 0)
        .column("失败率", "percent", 1),
        Panel::bars(
            "countries",
            "国家",
            at(0, 94, 12, 20),
            "rum_sessions",
            format!(
                r#"SELECT COALESCE("geo.country", "geo.country_iso_code") AS "国家", {TOTAL} AS "会话" FROM rum_sessions_latest WHERE {SCOPE} AND COALESCE("geo.country", "geo.country_iso_code") IS NOT NULL GROUP BY 1 ORDER BY 2 DESC LIMIT 5"#
            ),
        )
        .about("会话最多的 5 个国家。")
        .unit("short", 0),
        Panel::bars(
            "browsers",
            "浏览器",
            at(12, 94, 12, 20),
            "rum_sessions",
            format!(
                r#"SELECT "browser.name" AS "浏览器", {TOTAL} AS "会话" FROM rum_sessions_latest WHERE {SCOPE} AND "browser.name" IS NOT NULL GROUP BY 1 ORDER BY 2 DESC LIMIT 5"#
            ),
        )
        .about("会话最多的 5 种浏览器。")
        .unit("short", 0),
    ]
}
