// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 值班日历的订阅：生成和核对订阅 Token，把班次写成 RFC 5545 的日历（时间一律 UTC，文字按规范转义，
//! 长行按 75 个字节折行）。

use std::{collections::HashMap, sync::Arc};

use chrono::{TimeZone, Utc};

use crate::{
    app::alerting::tokens::{new_token, token_hash},
    domain::alerting::{
        calendar::{
            CALENDAR_FUTURE_MICROS, CALENDAR_PAST_MICROS, CalendarFeed, CalendarFeedRepository,
        },
        repositories::ScheduleRepository,
        schedule::Schedule,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 日历里写给人看的几句话，用订阅的人的语言；`{schedule}`、`{person}` 会被换掉。
#[derive(Debug, Clone)]
pub struct CalendarTexts {
    /// 个人订阅的日历名。
    pub person_name: String,
    /// 个人订阅里一个班次的标题，如「值班：{schedule}」。
    pub person_summary: String,
    /// 排班订阅里一个班次的标题，如「{person} 值班」。
    pub schedule_summary: String,
}

/// 日历里的一个班次。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarEvent {
    pub uid: String,
    pub start: TimestampMicros,
    pub end: TimestampMicros,
    pub summary: String,
    pub description: String,
}

pub struct CalendarFeeds {
    feeds: Arc<dyn CalendarFeedRepository>,
    schedules: Arc<dyn ScheduleRepository>,
}

impl CalendarFeeds {
    pub fn new(
        feeds: Arc<dyn CalendarFeedRepository>,
        schedules: Arc<dyn ScheduleRepository>,
    ) -> Self {
        Self { feeds, schedules }
    }

    pub async fn list(&self, org_id: &Id, user_id: &Id) -> Result<Vec<CalendarFeed>> {
        self.feeds.list_for_user(org_id, user_id).await
    }

    /// 新建或重置一份订阅，返回它和 Token；Token 只在这时给出。
    pub async fn create(
        &self,
        org_id: &Id,
        user_id: &Id,
        schedule_id: Option<Id>,
    ) -> Result<(CalendarFeed, String)> {
        if let Some(schedule_id) = &schedule_id {
            let schedule = self.schedules.get(schedule_id).await?;

            if &schedule.org_id != org_id {
                return Err(Error::not_found("schedule not found"));
            }
        }

        let token = new_token();
        let feed = self
            .feeds
            .replace(CalendarFeed {
                id: Id::new(),
                org_id: org_id.clone(),
                user_id: user_id.clone(),
                schedule_id,
                token_hash: token_hash(&token),
                created_at: TimestampMicros::now(),
                last_used_at: None,
            })
            .await?;

        Ok((feed, token))
    }

    pub async fn delete(&self, org_id: &Id, user_id: &Id, id: &Id) -> Result<bool> {
        self.feeds.delete(org_id, user_id, id).await
    }

    /// 地址里的 Token 对应的订阅。
    pub async fn find(&self, token: &str) -> Result<Option<CalendarFeed>> {
        let token = token.trim();

        if token.is_empty() || token.len() > 128 {
            return Ok(None);
        }

        self.feeds.find_by_token_hash(&token_hash(token)).await
    }

    /// 一份订阅现在的日历：名字和班次，班次从 30 天前到 90 天后。`names` 给值班人的名字，`link` 给排班
    /// 页面的地址。
    pub async fn calendar(
        &self,
        feed: &CalendarFeed,
        texts: &CalendarTexts,
        names: &HashMap<Id, String>,
        link: impl Fn(&Id) -> String,
        now: TimestampMicros,
    ) -> Result<(String, Vec<CalendarEvent>)> {
        self.feeds.touch(&feed.id, now).await?;

        let from = TimestampMicros(now.0 - CALENDAR_PAST_MICROS);
        let to = TimestampMicros(now.0 + CALENDAR_FUTURE_MICROS);

        let schedules: Vec<Schedule> = match &feed.schedule_id {
            Some(schedule_id) => {
                let schedule = self.schedules.get(schedule_id).await?;

                if schedule.org_id != feed.org_id {
                    return Err(Error::not_found("schedule not found"));
                }

                vec![schedule]
            }
            None => self.schedules.list(&feed.org_id).await?,
        };

        let mut events = Vec::new();

        for schedule in &schedules {
            for shift in schedule.shifts(from, to) {
                let summary = match &feed.schedule_id {
                    Some(_) => texts.schedule_summary.replace(
                        "{person}",
                        names
                            .get(&shift.user_id)
                            .map_or(shift.user_id.as_str(), String::as_str),
                    ),
                    None if shift.user_id == feed.user_id => {
                        texts.person_summary.replace("{schedule}", &schedule.name)
                    }
                    None => continue,
                };

                events.push(CalendarEvent {
                    uid: format!(
                        "{}-{}-{}@molesignal",
                        schedule.id, shift.start_at.0, shift.user_id
                    ),
                    start: shift.start_at,
                    end: shift.end_at,
                    summary,
                    description: link(&schedule.id),
                });
            }
        }

        events.sort_by_key(|event| event.start.0);

        let name = match &feed.schedule_id {
            Some(_) => schedules
                .first()
                .map(|schedule| schedule.name.clone())
                .unwrap_or_default(),
            None => texts.person_name.clone(),
        };

        Ok((name, events))
    }
}

/// 写成 RFC 5545 的一份日历。
pub fn render_ics(name: &str, events: &[CalendarEvent], now: TimestampMicros) -> String {
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".to_string(),
        "PRODID:-//MoleSignal//On-call//EN".to_string(),
        "CALSCALE:GREGORIAN".to_string(),
        "METHOD:PUBLISH".to_string(),
        format!("X-WR-CALNAME:{}", escape_text(name)),
        "REFRESH-INTERVAL;VALUE=DURATION:PT1H".to_string(),
        "X-PUBLISHED-TTL:PT1H".to_string(),
    ];

    for event in events {
        lines.extend([
            "BEGIN:VEVENT".to_string(),
            format!("UID:{}", escape_text(&event.uid)),
            format!("DTSTAMP:{}", ics_time(now)),
            format!("DTSTART:{}", ics_time(event.start)),
            format!("DTEND:{}", ics_time(event.end)),
            format!("SUMMARY:{}", escape_text(&event.summary)),
            format!("DESCRIPTION:{}", escape_text(&event.description)),
            "TRANSP:OPAQUE".to_string(),
            "END:VEVENT".to_string(),
        ]);
    }

    lines.push("END:VCALENDAR".to_string());

    lines
        .iter()
        .map(|line| fold(line))
        .collect::<Vec<_>>()
        .join("\r\n")
        + "\r\n"
}

fn ics_time(at: TimestampMicros) -> String {
    Utc.timestamp_micros(at.0)
        .single()
        .map(|time| time.format("%Y%m%dT%H%M%SZ").to_string())
        .unwrap_or_default()
}

fn escape_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace("\r\n", "\\n")
        .replace('\n', "\\n")
}

/// 超过 75 个字节的行在字符边界上折开，续行以一个空格开头。
fn fold(line: &str) -> String {
    let mut folded = String::with_capacity(line.len() + 8);
    let mut width = 0;

    for character in line.chars() {
        let size = character.len_utf8();

        if width + size > 75 {
            folded.push_str("\r\n ");
            width = 1;
        }

        folded.push(character);
        width += size;
    }

    folded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_calendar_is_written_by_the_standard() {
        let ics = render_ics(
            "我的值班",
            &[CalendarEvent {
                uid: "sch-1-0-u1@molesignal".into(),
                start: TimestampMicros(1_790_000_000_000_000),
                end: TimestampMicros(1_790_086_400_000_000),
                summary: "值班：支付, 主排班; 夜间".into(),
                description: "https://ms.example.com/alerts/schedules/sch-1".into(),
            }],
            TimestampMicros(1_789_000_000_000_000),
        );

        assert!(ics.starts_with("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n"));
        assert!(ics.ends_with("END:VCALENDAR\r\n"));
        assert!(ics.contains("DTSTART:20260921T"));
        assert!(ics.contains("SUMMARY:值班：支付\\, 主排班\\; 夜间"));
        assert!(
            ics.lines().all(|line| line.len() <= 76),
            "lines are folded at 75 bytes"
        );
    }

    #[test]
    fn long_lines_fold_on_character_boundaries() {
        let line = format!("SUMMARY:{}", "值".repeat(40));
        let folded = fold(&line);

        assert!(folded.contains("\r\n "));
        assert!(folded.split("\r\n").all(|part| part.len() <= 75));
        assert_eq!(folded.replace("\r\n ", ""), line);
    }
}
