// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PagerDuty 自己排出来的值班表：`final_schedule` 含临时替班，替班占的时段不比。

use serde::Deserialize;

use super::{
    super::{
        parse_time,
        rendered::{Rendered, RenderedShift},
    },
    Emails, Reference, Schedule,
};

#[derive(Deserialize)]
pub(super) struct SubSchedule {
    #[serde(default)]
    pub rendered_schedule_entries: Vec<Entry>,
}

#[derive(Deserialize)]
pub(super) struct Entry {
    start: String,
    end: String,
    #[serde(default)]
    user: Option<Reference>,
}

/// PagerDuty 自己排出来的值班表（`final_schedule`，含临时替班）。没有渲染结果时为空。
pub(super) fn rendered_of(
    schedule: &Schedule,
    overrides: &[Entry],
    emails: &Emails,
    window: Option<(i64, i64)>,
) -> Option<Rendered> {
    let entries = &schedule.final_schedule.as_ref()?.rendered_schedule_entries;

    let shifts: Vec<RenderedShift> = entries
        .iter()
        .filter_map(|entry| {
            let owner = entry.user.as_ref().map(|user| {
                emails
                    .get(&user.id)
                    .cloned()
                    .or_else(|| user.summary.clone())
                    .unwrap_or_else(|| user.id.clone())
            });

            Some(RenderedShift {
                start: parse_time(&entry.start)?,
                end: parse_time(&entry.end)?,
                owner,
            })
        })
        .collect();

    // 没有脚本给的范围时，只比最早一段到最晚一段之间。
    let (from, to) = window.or_else(|| {
        Some((
            shifts.iter().map(|shift| shift.start).min()?,
            shifts.iter().map(|shift| shift.end).max()?,
        ))
    })?;

    let excluded = overrides
        .iter()
        .filter_map(|entry| Some((parse_time(&entry.start)?, parse_time(&entry.end)?)))
        .collect();

    Some(Rendered {
        from,
        to,
        shifts,
        excluded,
    })
}
