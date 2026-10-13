// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! headless renderer 要加载的前端页面地址。
//!
//! 定时投递 worker、`GET /scheduled_reports/{id}/preview` 和资源分享的报表快照共用同一个
//! 地址。地址里除了报表引用的 dashboard / saved view，还带着报表自己的时间窗和时区：页面
//! 不读报表配置，不带上它们就只会按默认的最近 1 小时取数。

use chrono::{
    DateTime, Datelike, Days, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, TimeZone, Utc,
};
use chrono_tz::Tz;
use serde_json::Value;

use crate::{
    infra::persistence::repositories::scheduled_reports::ScheduledReport,
    shared::{Error, Result},
};

/// 报表在 `now` 这一刻要渲染的页面。
///
/// `report_render=1` 让前端只输出内容本身：没有应用外壳，文档随内容增高，renderer 按内容
/// 高度出图。
pub fn render_page_url(
    base_url: &str,
    report: &ScheduledReport,
    now: DateTime<Utc>,
) -> Result<String> {
    let base = base_url.trim().trim_end_matches('/');

    if base.is_empty() {
        return Err(Error::unavailable(
            "scheduled_reports.renderer.base_url is not configured",
        ));
    }

    let mut query = url::form_urlencoded::Serializer::new(String::new());

    let path = if let Some(id) = &report.dashboard_id {
        format!("/dashboards/{}", id.0)
    } else if let Some(id) = &report.saved_view_id {
        query.append_pair("view", &id.0);
        "/saved-views".to_owned()
    } else {
        return Err(Error::invalid(
            "report must reference a dashboard or saved view",
        ));
    };

    query.append_pair("report_render", "1");

    let zone = report_timezone(&report.time_range_json);

    if let Some((from, to)) = report_window(&report.time_range_json, zone, now) {
        query.append_pair("time", &format!("{from}..{to}"));
    }

    if let Some(zone) = zone {
        query.append_pair("report_tz", zone.name());
    }

    Ok(format!("{base}{path}?{}", query.finish()))
}

/// 报表的时区：自然日、周、月按它切分，页面上的时刻也按它显示。未设置或无法识别时为
/// `None`，页面按读者自己的时区显示，自然日按 UTC 切分。
fn report_timezone(time_range: &Value) -> Option<Tz> {
    time_range.get("timezone")?.as_str()?.trim().parse().ok()
}

/// 报表覆盖的时间窗，写成前端认识的时间表达式：滚动窗口是相对表达式（`now-7d`），自然日、
/// 周、月是报表时区下的绝对时刻。报表没有指明窗口（`custom`、未知预设、空值）时为
/// `None`，页面用它自己的默认窗口。
fn report_window(
    time_range: &Value,
    zone: Option<Tz>,
    now: DateTime<Utc>,
) -> Option<(String, String)> {
    time_range
        .get("preset")
        .and_then(Value::as_str)
        .and_then(|preset| preset_window(preset.trim(), zone.unwrap_or(Tz::UTC), now))
        .or_else(|| explicit_window(time_range))
}

/// 报表工作台保存的预设（`web/src/routes/reports/reportTypes.ts` 的 `RANGE_OPTIONS`）。
fn preset_window(preset: &str, zone: Tz, now: DateTime<Utc>) -> Option<(String, String)> {
    let today = now.with_timezone(&zone).date_naive();

    let (from, to) = match preset {
        "previous-1-hour" => return Some(rolling_window("1h")),
        "previous-24-hours" => return Some(rolling_window("24h")),
        "previous-7-days" => return Some(rolling_window("7d")),
        "previous-30-days" => return Some(rolling_window("30d")),
        "previous-calendar-day" => (today.checked_sub_days(Days::new(1))?, today),
        "previous-calendar-week" => {
            // 一周从周一开始（ISO 8601）。
            let into_week = u64::from(today.weekday().num_days_from_monday());
            let monday = today.checked_sub_days(Days::new(into_week))?;

            (monday.checked_sub_days(Days::new(7))?, monday)
        }
        "previous-calendar-month" => {
            let first = today.with_day(1)?;

            (first.checked_sub_days(Days::new(1))?.with_day(1)?, first)
        }
        _ => return None,
    };

    Some((
        instant(start_of_day(zone, from)),
        instant(start_of_day(zone, to)),
    ))
}

fn rolling_window(span: &str) -> (String, String) {
    (format!("now-{span}"), "now".to_owned())
}

/// 经 API 直接写入的报表可以用 `{from, to}` 给出窗口。两端都必须是页面能解析的表达式，
/// 否则宁可不带窗口，也不把原样文本拼进地址。
fn explicit_window(time_range: &Value) -> Option<(String, String)> {
    let from = time_expression(time_range.get("from")?)?;
    let to = time_expression(time_range.get("to")?)?;

    Some((from, to))
}

fn time_expression(value: &Value) -> Option<String> {
    let text = value.as_str()?.trim();

    if is_relative_expression(text) {
        return Some(text.to_owned());
    }

    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|at| instant(at.with_timezone(&Utc)))
}

/// `now` 或 `now-<数字><s|m|h|d>`，与前端 `resolveExpr` 的语法一致。
fn is_relative_expression(text: &str) -> bool {
    if text == "now" {
        return true;
    }

    let Some(span) = text.strip_prefix("now-") else {
        return false;
    };

    let Some(unit) = span.chars().last() else {
        return false;
    };

    let amount = &span[..span.len() - unit.len_utf8()];

    matches!(unit, 's' | 'm' | 'h' | 'd')
        && !amount.is_empty()
        && amount.bytes().all(|byte| byte.is_ascii_digit())
}

/// 某个时区里一天开始的时刻。夏令时切换会让某天的 00:00 不存在（当天从 01:00 开始）或
/// 出现两次，取当天最早存在的时刻。
fn start_of_day(zone: Tz, date: NaiveDate) -> DateTime<Utc> {
    let midnight = NaiveDateTime::new(date, NaiveTime::MIN);

    (0..=3)
        .find_map(|hours| {
            zone.from_local_datetime(&(midnight + TimeDelta::hours(hours)))
                .earliest()
        })
        .map_or_else(
            || Utc.from_utc_datetime(&midnight),
            |local| local.with_timezone(&Utc),
        )
}

fn instant(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::shared::{ids::Id, time::TimestampMicros};

    fn report(time_range: Value) -> ScheduledReport {
        ScheduledReport {
            id: Id("r1".into()),
            org_id: Id("orgA".into()),
            name: "weekly".into(),
            dashboard_id: Some(Id("d1".into())),
            saved_view_id: None,
            cron: "every:7d".into(),
            recipients: vec![],
            format: "pdf".into(),
            time_range_json: time_range,
            enabled: true,
            run_as_user_id: None,
            last_run_at: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn url_at(time_range: Value, now: &str) -> String {
        render_page_url("http://web:5173/", &report(time_range), at(now)).unwrap()
    }

    #[test]
    fn rolling_presets_stay_relative_and_carry_the_report_timezone() {
        let url = url_at(
            json!({ "preset": "previous-7-days", "timezone": "Asia/Shanghai" }),
            "2026-10-11T02:00:00Z",
        );

        assert_eq!(
            url,
            "http://web:5173/dashboards/d1?report_render=1&time=now-7d..now&report_tz=Asia%2FShanghai"
        );
    }

    #[test]
    fn calendar_week_runs_monday_to_monday_in_the_report_timezone() {
        // 上海时间周日 10:00：上一个自然周是 9 月 28 日（周一）到 10 月 5 日（周一）。
        let url = url_at(
            json!({ "preset": "previous-calendar-week", "timezone": "Asia/Shanghai" }),
            "2026-10-11T02:00:00Z",
        );

        assert!(
            url.contains("time=2026-09-27T16%3A00%3A00Z..2026-10-04T16%3A00%3A00Z"),
            "{url}"
        );
    }

    #[test]
    fn calendar_month_follows_the_local_date_not_the_utc_date() {
        // UTC 已是 3 月 1 日，纽约仍是 2 月 28 日：上一个自然月是 1 月。
        let url = url_at(
            json!({ "preset": "previous-calendar-month", "timezone": "America/New_York" }),
            "2026-03-01T00:30:00Z",
        );

        assert!(
            url.contains("time=2026-01-01T05%3A00%3A00Z..2026-02-01T05%3A00%3A00Z"),
            "{url}"
        );
    }

    #[test]
    fn calendar_day_is_a_local_day_across_a_daylight_saving_change() {
        // 纽约 3 月 8 日进入夏令时，这一天只有 23 小时。
        let url = url_at(
            json!({ "preset": "previous-calendar-day", "timezone": "America/New_York" }),
            "2026-03-09T12:00:00Z",
        );

        assert!(
            url.contains("time=2026-03-08T05%3A00%3A00Z..2026-03-09T04%3A00%3A00Z"),
            "{url}"
        );
    }

    #[test]
    fn calendar_presets_without_a_timezone_are_cut_in_utc() {
        let url = url_at(
            json!({ "preset": "previous-calendar-day" }),
            "2026-10-11T02:00:00Z",
        );

        assert_eq!(
            url,
            "http://web:5173/dashboards/d1?report_render=1&time=2026-10-10T00%3A00%3A00Z..2026-10-11T00%3A00%3A00Z"
        );
    }

    #[test]
    fn a_day_whose_midnight_does_not_exist_starts_at_its_first_moment() {
        // 圣保罗 2018-11-04 的 00:00 被夏令时跳过，这一天从 01:00（UTC-2）开始。
        let start = start_of_day(chrono_tz::America::Sao_Paulo, {
            NaiveDate::from_ymd_opt(2018, 11, 4).unwrap()
        });

        assert_eq!(instant(start), "2018-11-04T03:00:00Z");
    }

    #[test]
    fn explicit_bounds_are_normalised_and_unreadable_ones_are_dropped() {
        assert!(
            url_at(
                json!({ "from": "now-7d", "to": "now" }),
                "2026-10-11T02:00:00Z"
            )
            .ends_with("?report_render=1&time=now-7d..now")
        );
        assert!(
            url_at(
                json!({ "from": "2026-10-01T08:00:00+08:00", "to": "2026-10-02T08:00:00+08:00" }),
                "2026-10-11T02:00:00Z",
            )
            .contains("time=2026-10-01T00%3A00%3A00Z..2026-10-02T00%3A00%3A00Z")
        );

        for unreadable in [
            json!({ "from": "yesterday", "to": "now" }),
            json!({ "from": "now-7w", "to": "now" }),
            json!({ "from": "now-d", "to": "now" }),
            json!({ "from": "now-7d" }),
            json!({ "from": "now-7d&stack=x", "to": "now" }),
            json!({ "from": "now-7时", "to": "now" }),
        ] {
            let url = url_at(unreadable.clone(), "2026-10-11T02:00:00Z");

            assert_eq!(
                url, "http://web:5173/dashboards/d1?report_render=1",
                "{unreadable}"
            );
        }
    }

    #[test]
    fn a_report_without_a_stated_window_leaves_the_page_its_default() {
        for unstated in [
            json!({ "preset": "custom", "timezone": "UTC" }),
            json!({ "preset": "next-year" }),
            json!({}),
            Value::Null,
        ] {
            let url = url_at(unstated.clone(), "2026-10-11T02:00:00Z");

            assert!(!url.contains("time="), "{unstated}: {url}");
        }
    }

    #[test]
    fn timezone_names_are_encoded_and_unknown_ones_are_left_out() {
        let fixed = url_at(
            json!({ "preset": "previous-24-hours", "timezone": "Etc/GMT+8" }),
            "2026-10-11T02:00:00Z",
        );
        let unknown = url_at(
            json!({ "preset": "previous-24-hours", "timezone": "Mars/Olympus" }),
            "2026-10-11T02:00:00Z",
        );

        assert!(fixed.ends_with("&report_tz=Etc%2FGMT%2B8"), "{fixed}");
        assert!(unknown.ends_with("&time=now-24h..now"), "{unknown}");
    }

    #[test]
    fn saved_view_reports_name_the_view() {
        let mut saved = report(json!({ "preset": "previous-24-hours" }));
        saved.dashboard_id = None;
        saved.saved_view_id = Some(Id("v1".into()));

        let url = render_page_url("http://web:5173", &saved, at("2026-10-11T02:00:00Z")).unwrap();

        assert_eq!(
            url,
            "http://web:5173/saved-views?view=v1&report_render=1&time=now-24h..now"
        );
    }

    #[test]
    fn a_missing_base_url_or_source_is_an_error() {
        let now = at("2026-10-11T02:00:00Z");
        let mut sourceless = report(json!({}));
        sourceless.dashboard_id = None;

        assert!(matches!(
            render_page_url("  ", &report(json!({})), now),
            Err(Error::Unavailable(_))
        ));
        assert!(matches!(
            render_page_url("http://web:5173", &sourceless, now),
            Err(Error::InvalidArgument(_))
        ));
    }
}
