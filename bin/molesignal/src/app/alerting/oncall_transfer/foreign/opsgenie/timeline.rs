// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Opsgenie 自己排出来的值班表：`GET /v2/schedules/{id}/timeline` 的结果。

use serde::Deserialize;

use super::{
    super::{
        parse_time,
        rendered::{Rendered, RenderedShift},
    },
    Schedule,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Timeline {
    #[serde(rename = "_parent", default)]
    parent: Option<Parent>,
    start_date: String,
    end_date: String,
    #[serde(default)]
    base_timeline: Option<TimelineSection>,
    #[serde(default)]
    final_timeline: Option<TimelineSection>,
}

#[derive(Deserialize)]
struct Parent {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct TimelineSection {
    #[serde(default)]
    rotations: Vec<TimelineRotation>,
}

#[derive(Deserialize)]
struct TimelineRotation {
    #[serde(default)]
    periods: Vec<Period>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Period {
    start_date: String,
    end_date: String,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    recipient: Option<Recipient>,
}

#[derive(Deserialize)]
struct Recipient {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

pub(super) fn belongs_to(timeline: &Timeline, schedule: &Schedule) -> bool {
    let Some(parent) = &timeline.parent else {
        return false;
    };

    match (&parent.id, &schedule.id) {
        (Some(left), Some(right)) => left == right,
        _ => parent.name.is_some() && parent.name == schedule.name,
    }
}

/// Opsgenie 自己排出来的值班表。优先用不含替班和转发的 `baseTimeline`；只有 `finalTimeline` 时替班和转发
/// 的时段不比。
pub(super) fn rendered_of(timeline: &Timeline) -> Option<Rendered> {
    let section = timeline
        .base_timeline
        .as_ref()
        .or(timeline.final_timeline.as_ref())?;
    let periods = || {
        section
            .rotations
            .iter()
            .flat_map(|rotation| &rotation.periods)
    };
    let is_default = |period: &&Period| period.kind.as_deref().is_none_or(|kind| kind == "default");

    let shifts = periods()
        .filter(is_default)
        .filter_map(|period| {
            let owner = period
                .recipient
                .as_ref()
                .filter(|recipient| recipient.kind.as_deref() != Some("none"))
                .and_then(|recipient| recipient.name.clone());

            Some(RenderedShift {
                start: parse_time(&period.start_date)?,
                end: parse_time(&period.end_date)?,
                owner,
            })
        })
        .collect();

    let excluded = periods()
        .filter(|period| !is_default(period))
        .filter_map(|period| {
            Some((
                parse_time(&period.start_date)?,
                parse_time(&period.end_date)?,
            ))
        })
        .collect();

    Some(Rendered {
        from: parse_time(&timeline.start_date)?,
        to: parse_time(&timeline.end_date)?,
        shifts,
        excluded,
    })
}
