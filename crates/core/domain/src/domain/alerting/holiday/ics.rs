// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 读 ICS（iCalendar）里的节假日：每个 `VEVENT` 是一天或连续几天，`DTEND` 不含在内；名称里带
//! 「补班」「上班」或以「班」结尾的算补班，其余算放假。重复规则（`RRULE`）不展开。
//!
//! 只是给导入预览用：结果先给人看、能改，确认后才存成日历。

use chrono::{Duration, NaiveDate};

use super::{DayKind, HolidayDay, MAX_CALENDAR_DAYS, MAX_DAY_NAME_CHARS};
use crate::shared::{Error, Result};

/// 一个事件最多连着几天：再长多半是读错了。
const MAX_EVENT_DAYS: i64 = 31;

/// 读出 ICS 里的每一天，按日期排好；同一天出现几次只留第一次。
pub fn parse_ics(text: &str) -> Result<Vec<HolidayDay>> {
    let mut days: Vec<HolidayDay> = Vec::new();
    let mut event: Option<Event> = None;
    let mut saw_calendar = false;

    for line in unfold(text) {
        let (name, value) = match line.split_once(':') {
            Some((name, value)) => (name, value),
            None => continue,
        };
        let property = name.split(';').next().unwrap_or(name).to_ascii_uppercase();

        match (property.as_str(), value.trim()) {
            ("BEGIN", "VCALENDAR") => saw_calendar = true,
            ("BEGIN", "VEVENT") => event = Some(Event::default()),
            ("END", "VEVENT") => {
                if let Some(done) = event.take() {
                    done.days_into(&mut days)?;
                }
            }
            ("DTSTART", value) => {
                if let Some(event) = event.as_mut() {
                    event.start = date_of(value);
                }
            }
            ("DTEND", value) => {
                if let Some(event) = event.as_mut() {
                    event.end = date_of(value);
                }
            }
            ("SUMMARY", value) => {
                if let Some(event) = event.as_mut() {
                    event.summary = unescape(value);
                }
            }
            _ => {}
        }

        if days.len() > MAX_CALENDAR_DAYS {
            return Err(Error::invalid(format!(
                "the file has more than {MAX_CALENDAR_DAYS} days"
            )));
        }
    }

    if !saw_calendar {
        return Err(Error::invalid("this is not an iCalendar (ICS) file"));
    }

    days.sort_by_key(|day| day.date);
    days.dedup_by_key(|day| day.date);
    Ok(days)
}

#[derive(Default)]
struct Event {
    start: Option<NaiveDate>,
    end: Option<NaiveDate>,
    summary: String,
}

impl Event {
    fn days_into(self, days: &mut Vec<HolidayDay>) -> Result<()> {
        let Some(start) = self.start else {
            return Ok(());
        };

        // 没写结束，或结束不晚于开始，就是一天。
        let end = self
            .end
            .filter(|end| *end > start)
            .unwrap_or(start + Duration::days(1));

        if (end - start).num_days() > MAX_EVENT_DAYS {
            return Err(Error::invalid(format!(
                "`{}` lasts more than {MAX_EVENT_DAYS} days",
                self.summary
            )));
        }

        let kind = kind_of(&self.summary);
        let name: String = self.summary.chars().take(MAX_DAY_NAME_CHARS).collect();

        for date in start.iter_days().take_while(|date| *date < end) {
            days.push(HolidayDay {
                date,
                kind,
                name: name.clone(),
            });
        }

        Ok(())
    }
}

/// 名称里带「补班」「上班」，或以「班」结尾（如「国庆节（班）」）的是补班。
fn kind_of(summary: &str) -> DayKind {
    let trimmed = summary.trim().trim_end_matches([')', '）']);

    if summary.contains("补班") || summary.contains("上班") || trimmed.ends_with('班') {
        DayKind::Workday
    } else {
        DayKind::Holiday
    }
}

/// `20261001`、`20261001T000000Z` 都取日期。
fn date_of(value: &str) -> Option<NaiveDate> {
    let digits = value.get(..8)?;
    NaiveDate::parse_from_str(digits, "%Y%m%d").ok()
}

/// 折行：下一行以空格或制表符开头时接在上一行后面（去掉那个空白）。
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();

    for raw in text.lines() {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);

        match (raw.strip_prefix([' ', '\t']), lines.last_mut()) {
            (Some(rest), Some(last)) => last.push_str(rest),
            _ => lines.push(raw.to_string()),
        }
    }

    lines
}

/// 文本值里的 `\,`、`\;`、`\n`、`\\`。
fn unescape(value: &str) -> String {
    let mut text = String::with_capacity(value.len());
    let mut chars = value.chars();

    while let Some(c) = chars.next() {
        if c != '\\' {
            text.push(c);
            continue;
        }

        match chars.next() {
            Some('n' | 'N') => text.push(' '),
            Some(other) => text.push(other),
            None => {}
        }
    }

    text.trim().to_string()
}
