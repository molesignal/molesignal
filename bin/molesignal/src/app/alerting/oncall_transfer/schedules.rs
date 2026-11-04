// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 包里的一项（名字已经换回 id）变成要写的排班。

use serde::Deserialize;
use serde_json::Value;

use crate::{
    domain::alerting::schedule::{Rotation, Schedule},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 导出时不带的键。
pub const SCHEDULE_DROP: [&str; 7] = [
    "id",
    "orgId",
    "overrides",
    "createdBy",
    "updatedBy",
    "createdAt",
    "updatedAt",
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleWrite {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    team_id: Option<Id>,
    #[serde(default = "utc")]
    timezone: String,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    rotations: Vec<Rotation>,
    #[serde(default)]
    swap_needs_approval: Option<bool>,
}

fn utc() -> String {
    "UTC".into()
}

/// 这次导入是谁、往哪个组织、什么时候。
pub struct ScheduleScope<'a> {
    pub org_id: &'a Id,
    pub user_id: &'a Id,
    pub now: TimestampMicros,
}

/// 照保存时那样检查。`id`、`before` 是要更新的那一项：更新时留着它的替班、换班和临时覆盖、谁建的，以及
/// 文件没写的设置（PagerDuty 的响应就没有 `enabled`，不该把暂停的排班打开）。
pub fn schedule_from_item(
    value: Value,
    id: Option<&Id>,
    before: Option<&Schedule>,
    scope: &ScheduleScope<'_>,
) -> Result<Schedule> {
    // `teamId: null` 是说没有团队，没写才是没说。
    let names_team = value.get("teamId").is_some();
    let write: ScheduleWrite = serde_json::from_value(value)
        .map_err(|error| Error::invalid(format!("not a schedule: {error}")))?;

    if write.name.trim().is_empty() {
        return Err(Error::invalid("name cannot be empty"));
    }

    let schedule = Schedule {
        id: id.cloned().unwrap_or_default(),
        org_id: scope.org_id.clone(),
        name: write.name.trim().to_string(),
        description: write
            .description
            .or_else(|| before.map(|schedule| schedule.description.clone()))
            .unwrap_or_default(),
        team_id: if names_team {
            write.team_id
        } else {
            before.and_then(|schedule| schedule.team_id.clone())
        },
        timezone: write.timezone,
        enabled: write
            .enabled
            .or_else(|| before.map(|schedule| schedule.enabled))
            .unwrap_or(true),
        rotations: write.rotations,
        overrides: before
            .map(|schedule| schedule.overrides.clone())
            .unwrap_or_default(),
        swap_needs_approval: write
            .swap_needs_approval
            .or_else(|| before.map(|schedule| schedule.swap_needs_approval))
            .unwrap_or_default(),
        created_by: before.map_or(Some(scope.user_id.clone()), |schedule| {
            schedule.created_by.clone()
        }),
        updated_by: Some(scope.user_id.clone()),
        created_at: before.map_or(scope.now, |schedule| schedule.created_at),
        updated_at: scope.now,
    };

    schedule.validate()?;
    schedule.check_calendars()?;
    Ok(schedule)
}
