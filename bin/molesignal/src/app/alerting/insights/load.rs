// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 值班负担：每个人在一段时间里值了多久的班、收到几次告警通知，其中几次在夜里、几次在周末，被吵醒
//! 了几个晚上。
//!
//! 夜里是本地 [`NIGHT_START_HOUR`] 点到次日 [`NIGHT_END_HOUR`] 点，按这个人所在排班的时区；不在任何
//! 排班里的，按他免打扰时段的时区，再没有就按 UTC。凌晨的通知算前一个晚上。

use std::collections::{BTreeSet, HashMap};

use chrono::{Datelike, Duration, TimeZone, Timelike, Utc, Weekday};
use serde::Serialize;

use crate::{
    domain::{alerting::schedule::Schedule, notify::delivery::PageRecord},
    shared::{ids::Id, time::TimestampMicros},
};

pub const NIGHT_START_HOUR: u32 = 22;
pub const NIGHT_END_HOUR: u32 = 8;

/// 一个人的值班负担。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonLoad {
    pub user_id: Id,
    pub oncall_secs: i64,
    pub pages: u32,
    pub night_pages: u32,
    pub weekend_pages: u32,
    pub interrupted_nights: u32,
    /// 判断夜里、周末用的时区。
    pub timezone: String,
    /// 这段时间里他先接手的事件数，和平均多久接手（秒）。
    pub responded: u32,
    pub mtta_secs: f64,
}

/// `[from, to)` 里每个人的值班负担，通知多的在前。`only` 给了排班时，只算这个排班的成员和班次。
pub fn oncall_load(
    schedules: &[Schedule],
    pages: &[PageRecord],
    quiet_zones: &HashMap<Id, String>,
    from: TimestampMicros,
    to: TimestampMicros,
    only: Option<&Id>,
) -> Vec<PersonLoad> {
    let schedules: Vec<&Schedule> = schedules
        .iter()
        .filter(|schedule| only.is_none_or(|id| &schedule.id == id))
        .collect();

    let mut people: HashMap<Id, PersonLoad> = HashMap::new();

    for schedule in &schedules {
        let members = schedule
            .rotations
            .iter()
            .flat_map(|rotation| rotation.members.iter())
            .chain(schedule.overrides.iter().map(|cover| &cover.user_id));

        for member in members {
            person(&mut people, member, Some(&schedule.timezone));
        }

        for shift in schedule.shifts(from, to) {
            person(&mut people, &shift.user_id, Some(&schedule.timezone)).oncall_secs +=
                (shift.end_at.0 - shift.start_at.0) / 1_000_000;
        }
    }

    let mut nights: HashMap<Id, BTreeSet<(i32, u32)>> = HashMap::new();

    for page in pages {
        if only.is_some() && !people.contains_key(&page.user_id) {
            continue;
        }

        let quiet = quiet_zones.get(&page.user_id).map(String::as_str);
        let load = person(&mut people, &page.user_id, quiet);

        if load.timezone.is_empty() {
            load.timezone = "UTC".into();
        }

        let zone = load
            .timezone
            .parse::<chrono_tz::Tz>()
            .unwrap_or(chrono_tz::UTC);

        let Some(local) = Utc
            .timestamp_micros(page.at.0)
            .single()
            .map(|at| at.with_timezone(&zone))
        else {
            continue;
        };

        load.pages += 1;

        if matches!(local.weekday(), Weekday::Sat | Weekday::Sun) {
            load.weekend_pages += 1;
        }

        let hour = local.hour();

        if !(NIGHT_END_HOUR..NIGHT_START_HOUR).contains(&hour) {
            load.night_pages += 1;

            let night = if hour < NIGHT_END_HOUR {
                local.date_naive() - Duration::days(1)
            } else {
                local.date_naive()
            };

            nights
                .entry(page.user_id.clone())
                .or_default()
                .insert((night.year(), night.ordinal()));
        }
    }

    for (user_id, set) in nights {
        if let Some(load) = people.get_mut(&user_id) {
            load.interrupted_nights = u32::try_from(set.len()).unwrap_or(u32::MAX);
        }
    }

    let mut loads: Vec<PersonLoad> = people
        .into_values()
        .map(|mut load| {
            if load.timezone.is_empty() {
                load.timezone = "UTC".into();
            }

            load
        })
        .collect();

    loads.sort_by(|a, b| {
        b.pages
            .cmp(&a.pages)
            .then_with(|| b.oncall_secs.cmp(&a.oncall_secs))
            .then_with(|| a.user_id.0.cmp(&b.user_id.0))
    });

    loads
}

/// 这个人的那一行，没有就加上；还不知道时区时记下 `timezone`。
fn person<'a>(
    people: &'a mut HashMap<Id, PersonLoad>,
    user_id: &Id,
    timezone: Option<&str>,
) -> &'a mut PersonLoad {
    let load = people.entry(user_id.clone()).or_insert_with(|| PersonLoad {
        user_id: user_id.clone(),
        oncall_secs: 0,
        pages: 0,
        night_pages: 0,
        weekend_pages: 0,
        interrupted_nights: 0,
        timezone: String::new(),
        responded: 0,
        mtta_secs: 0.0,
    });

    if load.timezone.is_empty()
        && let Some(timezone) = timezone.filter(|zone| !zone.is_empty())
    {
        load.timezone = timezone.to_string();
    }

    load
}

#[cfg(test)]
#[path = "load_tests.rs"]
mod tests;
