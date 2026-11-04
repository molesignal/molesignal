// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! On-call 排班。
//!
//! 一个 [`Schedule`] 由一条或多条 [`Rotation`] 拼接而成，
//! 通过 [`ScheduleOverride`] 可临时替换某个时间段的 on-call 人。
//!
//! 解析"当前 on-call 是谁"是一个纯函数 `who_is_on_call(schedule, at)`：
//! 1. 若 `at` 落在某个 Override 内 → 直接返回该 Override 指定的人
//! 2. 否则按 Rotation 的周期算出当前班次序号 → 取对应成员
//!
//! 轮换按排班时区的本地时间推进：第 k 次交接是起始时刻的本地时间加 k 个周期，所以有夏令时的时区里
//! 交接时刻全年不变，跨夏令时的那一班长一小时或短一小时。生效时段也按本地时间判断，精确到分钟。

use chrono::Duration;
use serde::{Deserialize, Serialize};

use super::time_window::{
    ALL_DAY, CalendarUse, WeeklyRange, local_instant, parse_timezone, timezone_or_utc,
    validate_ranges, windows_contain, windows_transitions,
};
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

/// 求覆盖时段时，一条轮值最多列出多少次交接；周期极短的轮值不会因此列出几十万个点。
const MAX_HANDOFFS: usize = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    /// 面向值班人员的用途说明。
    #[serde(default)]
    pub description: String,
    /// 可选关联团队，用于列表筛选与责任归属展示。
    #[serde(default)]
    pub team_id: Option<Id>,
    /// 时区名（IANA），轮换的交接时刻与生效时段都按它的本地时间。
    pub timezone: String,
    /// 暂停的排班不参与 on-call 解析。
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub rotations: Vec<Rotation>,
    pub overrides: Vec<ScheduleOverride>,
    /// 换班申请对方接受后，还要有排班管理权限的人批准。
    #[serde(default)]
    pub swap_needs_approval: bool,
    #[serde(default)]
    pub created_by: Option<Id>,
    #[serde(default)]
    pub updated_by: Option<Id>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rotation {
    pub id: Id,
    pub name: String,
    /// 参与轮值的成员，按顺序轮换。
    pub members: Vec<Id>,
    pub kind: RotationKind,
    /// 只在这些时段生效（排班时区的本地时间），例如工作日 09:30–18:00；空着时一直生效。
    #[serde(default)]
    pub active_windows: Vec<WeeklyRange>,
    /// 生效时段按这份节假日日历算：节假日不上班，或只在节假日上班；没设生效时段时按全天。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calendar: Option<CalendarUse>,
    /// 轮值起算时间：第一次交接，之后每次交接都在它的本地时刻。
    pub start_at: TimestampMicros,
    /// 轮值到这一刻结束，从这一刻起这条轮值没人值班；空着时一直轮下去。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_at: Option<TimestampMicros>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum RotationKind {
    Daily,
    Weekly,
    /// 自定义周期（秒）
    Custom {
        period_secs: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleOverride {
    pub id: Id,
    pub user_id: Id,
    pub start_at: TimestampMicros,
    pub end_at: TimestampMicros,
    pub reason: String,
}

/// 某一刻由谁值班、因为什么。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub user_id: Id,
    pub source: CoverageSource,
    pub rotation_id: Option<Id>,
    pub override_id: Option<Id>,
}

/// 覆盖时段的来历。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageSource {
    Rotation,
    Override,
    /// 没人值班。
    Gap,
}

/// 一段由同一来历、同一个人值的时间；没人值班的一段也列出来。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageSegment {
    pub start_at: TimestampMicros,
    pub end_at: TimestampMicros,
    pub source: CoverageSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation_id: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub override_id: Option<Id>,
    /// 替班的时段里，按轮值本该值班的人；没人时为空。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaced_user_id: Option<Id>,
}

/// 一段连着由同一个人值的班。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shift {
    pub user_id: Id,
    pub start_at: TimestampMicros,
    pub end_at: TimestampMicros,
}

impl Schedule {
    /// 时区认得出；每条轮值都说得通。
    pub fn validate(&self) -> Result<()> {
        parse_timezone(&self.timezone)?;
        self.rotations.iter().try_for_each(Rotation::validate)
    }

    /// 轮值引用的节假日日历都是这个组织的。
    pub fn check_calendars(&self) -> Result<()> {
        self.rotations
            .iter()
            .filter_map(|rotation| rotation.calendar.as_ref())
            .try_for_each(|calendar| calendar.check(&self.org_id))
    }

    /// 解析在 `at` 时刻当前 on-call 是谁。
    /// 没人值班时返回 `None`。
    pub fn who_is_on_call(&self, at: TimestampMicros) -> Option<Id> {
        self.resolution_at(at).map(|resolution| resolution.user_id)
    }

    /// 在 `at` 时刻由谁值班、因为哪个替班或哪条轮值；没人值班或排班暂停时为空。
    pub fn resolution_at(&self, at: TimestampMicros) -> Option<Resolution> {
        if !self.enabled {
            return None;
        }

        // 1. Override 优先
        if let Some(cover) = self
            .overrides
            .iter()
            .find(|cover| cover.start_at.0 <= at.0 && at.0 < cover.end_at.0)
        {
            return Some(Resolution {
                user_id: cover.user_id.clone(),
                source: CoverageSource::Override,
                rotation_id: None,
                override_id: Some(cover.id.clone()),
            });
        }

        // 2. 否则按 rotation，排在前面的先算
        self.rotation_resolution(at)
    }

    /// 不看替班时，`at` 时按轮值由谁值班。
    fn rotation_resolution(&self, at: TimestampMicros) -> Option<Resolution> {
        let tz = timezone_or_utc(&self.timezone);

        self.rotations.iter().find_map(|rotation| {
            rotation.resolve_in_tz(at, tz).map(|user_id| Resolution {
                user_id,
                source: CoverageSource::Rotation,
                rotation_id: Some(rotation.id.clone()),
                override_id: None,
            })
        })
    }

    /// `[from, to)` 的覆盖时段，按时间先后，首尾相接；替班的起止、轮换的交接和生效时段的开关都会切开
    /// 时段，来历和人都相同的相邻时段合成一段。暂停的排班整段没人值班。
    pub fn segments(&self, from: TimestampMicros, to: TimestampMicros) -> Vec<CoverageSegment> {
        if to.0 <= from.0 {
            return Vec::new();
        }

        let mut points = vec![from.0];

        if self.enabled {
            let tz = timezone_or_utc(&self.timezone);

            for cover in &self.overrides {
                points.extend(
                    [cover.start_at.0, cover.end_at.0]
                        .into_iter()
                        .filter(|at| *at > from.0 && *at < to.0),
                );
            }

            for rotation in &self.rotations {
                points.extend(rotation.boundaries(from.0, to.0, tz));
            }
        }

        points.sort_unstable();
        points.dedup();

        let mut segments: Vec<CoverageSegment> = Vec::new();

        for (index, &start) in points.iter().enumerate() {
            let end = points.get(index + 1).copied().unwrap_or(to.0);
            let resolution = self.resolution_at(TimestampMicros(start));

            let replaced_user_id = resolution
                .as_ref()
                .filter(|resolution| resolution.source == CoverageSource::Override)
                .and_then(|_| self.rotation_resolution(TimestampMicros(start)))
                .map(|rotation| rotation.user_id);

            let segment = CoverageSegment {
                start_at: TimestampMicros(start),
                end_at: TimestampMicros(end),
                source: resolution
                    .as_ref()
                    .map_or(CoverageSource::Gap, |resolution| resolution.source),
                user_id: resolution
                    .as_ref()
                    .map(|resolution| resolution.user_id.clone()),
                rotation_id: resolution
                    .as_ref()
                    .and_then(|resolution| resolution.rotation_id.clone()),
                override_id: resolution.and_then(|resolution| resolution.override_id),
                replaced_user_id,
            };

            match segments.last_mut() {
                Some(last) if same_coverage(last, &segment) => last.end_at = segment.end_at,
                _ => segments.push(segment),
            }
        }

        segments
    }

    /// `[from, to)` 里的班次，按时间先后：同一个人连着值的合成一段（不管来自替班还是轮值）；
    /// 暂停的排班没有班次。
    pub fn shifts(&self, from: TimestampMicros, to: TimestampMicros) -> Vec<Shift> {
        let mut shifts: Vec<Shift> = Vec::new();

        for segment in self.segments(from, to) {
            let Some(user_id) = segment.user_id else {
                continue;
            };

            match shifts.last_mut() {
                Some(last) if last.user_id == user_id && last.end_at == segment.start_at => {
                    last.end_at = segment.end_at;
                }
                _ => shifts.push(Shift {
                    user_id,
                    start_at: segment.start_at,
                    end_at: segment.end_at,
                }),
            }
        }

        shifts
    }
}

fn same_coverage(last: &CoverageSegment, next: &CoverageSegment) -> bool {
    last.end_at == next.start_at
        && last.source == next.source
        && last.user_id == next.user_id
        && last.rotation_id == next.rotation_id
        && last.override_id == next.override_id
        && last.replaced_user_id == next.replaced_user_id
}

impl Rotation {
    /// 自定义周期至少 1 秒；结束晚于开始；生效时段说得通。
    pub fn validate(&self) -> Result<()> {
        if let RotationKind::Custom { period_secs: 0 } = self.kind {
            return Err(Error::invalid(
                "a custom rotation period must be at least one second",
            ));
        }

        if self.end_at.is_some_and(|end| end.0 <= self.start_at.0) {
            return Err(Error::invalid("a rotation must end after it starts"));
        }

        if let Some(calendar) = &self.calendar {
            calendar.validate()?;
        }

        validate_ranges(&self.active_windows)
    }

    /// 生效时段在什么时候算：设了的段，或（只设了节假日日历时）每天全天；都没设时一直生效。
    fn active_ranges(&self) -> Option<&[WeeklyRange]> {
        match (self.active_windows.is_empty(), &self.calendar) {
            (false, _) => Some(&self.active_windows),
            (true, Some(_)) => Some(std::slice::from_ref(&ALL_DAY)),
            (true, None) => None,
        }
    }

    /// 在 `tz` 时区下解析当前 on-call：第几班按本地时间的交接算，生效时段也按本地时间判断。
    pub fn resolve_in_tz(&self, at: TimestampMicros, tz: chrono_tz::Tz) -> Option<Id> {
        if self.members.is_empty() || self.end_at.is_some_and(|end| at.0 >= end.0) {
            return None;
        }

        let index = self.shift_index(at.0, tz)?;

        if let Some(ranges) = self.active_ranges()
            && !windows_contain(ranges, tz, self.calendar.as_ref(), at)
        {
            return None;
        }

        let position = index.rem_euclid(self.members.len() as i64) as usize;

        Some(self.members[position].clone())
    }

    /// UTC 版本。
    pub fn resolve(&self, at: TimestampMicros) -> Option<Id> {
        self.resolve_in_tz(at, chrono_tz::UTC)
    }

    fn period_micros(&self) -> i64 {
        match self.kind {
            RotationKind::Daily => 86_400_000_000,
            RotationKind::Weekly => 7 * 86_400_000_000,
            RotationKind::Custom { period_secs } => i64::try_from(period_secs)
                .unwrap_or(i64::MAX)
                .saturating_mul(1_000_000),
        }
    }

    /// 第 `index` 次交接的时刻（第 0 次是起始时刻）：起始时刻的本地时间加 `index` 个周期，再按时区
    /// 换回时刻。
    pub fn handoff(&self, index: i64, tz: chrono_tz::Tz) -> i64 {
        let local = self.start_at.to_datetime().with_timezone(&tz).naive_local();

        let step = match self.kind {
            RotationKind::Daily => Duration::try_days(index),
            RotationKind::Weekly => Duration::try_weeks(index),
            RotationKind::Custom { period_secs } => i64::try_from(period_secs)
                .ok()
                .and_then(|secs| secs.checked_mul(index))
                .and_then(Duration::try_seconds),
        };

        step.and_then(|step| local.checked_add_signed(step))
            .map_or(i64::MAX, |shifted| {
                local_instant(tz, shifted).timestamp_micros()
            })
    }

    /// `at` 时是第几班（从 0 起）：到 `at` 为止交接过几次。早于起始时刻时为空。
    pub fn shift_index(&self, at: i64, tz: chrono_tz::Tz) -> Option<i64> {
        if at < self.start_at.0 {
            return None;
        }

        // 本地时间和 UTC 只差夏令时那一点：先按 UTC 估，再往前后校正。
        let mut index = (at - self.start_at.0) / self.period_micros().max(1);

        while index > 0 && self.handoff(index, tz) > at {
            index -= 1;
        }

        while self.handoff(index + 1, tz) <= at {
            index += 1;
        }

        Some(index)
    }

    /// `(from, to)` 里可能换人的时刻：开始轮值、每次交接、生效时段的开关、结束轮值。
    fn boundaries(&self, from: i64, to: i64, tz: chrono_tz::Tz) -> Vec<i64> {
        let mut points = Vec::new();

        if self.start_at.0 > from && self.start_at.0 < to {
            points.push(self.start_at.0);
        }

        // 结束之后不再换人。
        let to = match self.end_at {
            Some(end) if end.0 <= from => return points,
            Some(end) if end.0 < to => {
                points.push(end.0);
                end.0
            }
            _ => to,
        };

        if let Some(mut index) = self.shift_index(from.max(self.start_at.0), tz) {
            for _ in 0..MAX_HANDOFFS {
                index += 1;
                let at = self.handoff(index, tz);

                if at >= to {
                    break;
                }

                if at > from {
                    points.push(at);
                }
            }
        }

        if let Some(ranges) = self.active_ranges() {
            points.extend(windows_transitions(
                ranges,
                tz,
                self.calendar.as_ref(),
                from,
                to,
            ));
        }

        points
    }
}

#[cfg(test)]
mod tests;
