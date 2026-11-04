// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 节假日日历：一个组织可以有几份（如「中国大陆」「美国」），每份是若干天，每天是放假或补班。
//!
//! 日期是本地日期，按引用它的时间段（或排班）自己的时区算哪一天。时间段和轮值引用一份日历
//! （[`super::time_window::CalendarUse`]）：节假日不算在时段里，或只在节假日才算；补班日按周一的
//! 时段算。判断时读进程里的缓存（[`book`]），调用处不用传日历。

mod book;
mod ics;

use async_trait::async_trait;
pub use book::{belongs_to, day_kind, forget, remember, replace_all};
use chrono::NaiveDate;
pub use ics::parse_ics;
use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

/// 日历名称最多几个字。
pub const MAX_CALENDAR_NAME_CHARS: usize = 80;

/// 说明最多几个字。
pub const MAX_CALENDAR_DESCRIPTION_CHARS: usize = 500;

/// 一天的名称最多几个字。
pub const MAX_DAY_NAME_CHARS: usize = 80;

/// 一份日历最多几天：二十年的节假日和补班也够。
pub const MAX_CALENDAR_DAYS: usize = 2000;

/// 这一天是放假还是补班。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DayKind {
    Holiday,
    /// 本来不上班的日子要上班（调休）：按周一的时段算。
    Workday,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HolidayDay {
    /// `YYYY-MM-DD`。
    pub date: NaiveDate,
    pub kind: DayKind,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HolidayCalendar {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub days: Vec<HolidayDay>,
    #[serde(default)]
    pub created_by: Option<Id>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl HolidayCalendar {
    /// 去掉名称两边的空白、按日期排好；名称不空，各项不超长，同一天不能写两次。
    pub fn normalize(&mut self) -> Result<()> {
        self.name = self.name.trim().to_string();
        self.description = self.description.trim().to_string();

        if self.name.is_empty() {
            return Err(Error::invalid("name cannot be empty"));
        }

        if self.name.chars().count() > MAX_CALENDAR_NAME_CHARS {
            return Err(Error::invalid(format!(
                "name has at most {MAX_CALENDAR_NAME_CHARS} characters"
            )));
        }

        if self.description.chars().count() > MAX_CALENDAR_DESCRIPTION_CHARS {
            return Err(Error::invalid(format!(
                "description has at most {MAX_CALENDAR_DESCRIPTION_CHARS} characters"
            )));
        }

        if self.days.len() > MAX_CALENDAR_DAYS {
            return Err(Error::invalid(format!(
                "a holiday calendar has at most {MAX_CALENDAR_DAYS} days"
            )));
        }

        for day in &mut self.days {
            day.name = day.name.trim().to_string();

            if day.name.chars().count() > MAX_DAY_NAME_CHARS {
                return Err(Error::invalid(format!(
                    "the name of {} has at most {MAX_DAY_NAME_CHARS} characters",
                    day.date
                )));
            }
        }

        self.days.sort_by_key(|day| day.date);

        if let Some(pair) = self
            .days
            .windows(2)
            .find(|pair| pair[0].date == pair[1].date)
        {
            return Err(Error::invalid(format!(
                "{} is in the calendar twice",
                pair[0].date
            )));
        }

        Ok(())
    }
}

/// 一个引用了日历的地方，删日历前列给人看。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CalendarReference {
    /// `routing_rule`、`escalation_policy`、`schedule`、`mute_rule`、`notify_policy`、`alert_pipeline`。
    pub kind: String,
    pub id: Id,
    pub name: String,
}

#[async_trait]
pub trait HolidayCalendarRepository: Send + Sync {
    async fn create(&self, calendar: HolidayCalendar) -> Result<HolidayCalendar>;
    async fn update(&self, calendar: HolidayCalendar) -> Result<HolidayCalendar>;
    async fn get(&self, id: &Id) -> Result<HolidayCalendar>;
    async fn list(&self, org_id: &Id) -> Result<Vec<HolidayCalendar>>;
    /// 全部组织的日历：给进程里的缓存用。
    async fn list_all(&self) -> Result<Vec<HolidayCalendar>>;
    async fn delete(&self, id: &Id) -> Result<()>;
    /// 这个组织里哪些地方引用了这份日历。
    async fn references(&self, org_id: &Id, id: &Id) -> Result<Vec<CalendarReference>>;
}

#[cfg(test)]
mod tests;
