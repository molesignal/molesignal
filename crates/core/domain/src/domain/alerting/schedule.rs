// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! On-call 排班。
//!
//! 一个 [`Schedule`] 由一条或多条 [`Rotation`] 拼接而成，
//! 通过 [`Override`] 可临时替换某个时间段的 on-call 人。
//!
//! 解析"当前 on-call 是谁"是一个纯函数 `who_is_on_call(schedule, at)`：
//! 1. 若 `at` 落在某个 Override 内 → 直接返回该 Override 指定的人
//! 2. 否则按 Rotation 的周期算出当前班次序号 → 取对应成员

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::shared::{ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// 时区名（IANA），影响周期切换时刻。
    pub timezone: String,
    /// 暂停的排班不参与 on-call 解析。
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub rotations: Vec<Rotation>,
    pub overrides: Vec<ScheduleOverride>,
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
pub struct Rotation {
    pub id: Id,
    pub name: String,
    /// 参与轮值的成员，按顺序轮换。
    pub members: Vec<Id>,
    pub kind: RotationKind,
    /// 该轮值生效的时间窗（小时级，0-23）和星期掩码，可表达"工作日 9-18"等。
    pub active_window: Option<ActiveWindow>,
    /// 轮值起算时间。
    pub start_at: TimestampMicros,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RotationKind {
    Daily,
    Weekly,
    /// 自定义周期（秒）
    Custom {
        period_secs: u64,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ActiveWindow {
    /// 0=周日，6=周六
    pub weekday_mask: u8,
    pub hour_start: u8,
    pub hour_end: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleOverride {
    pub id: Id,
    pub user_id: Id,
    pub start_at: TimestampMicros,
    pub end_at: TimestampMicros,
    pub reason: String,
}

impl Schedule {
    /// 解析在 `at` 时刻当前 on-call 是谁。
    /// 没人值班时返回 `None`。
    pub fn who_is_on_call(&self, at: TimestampMicros) -> Option<Id> {
        if !self.enabled {
            return None;
        }
        // 1. Override 优先
        for ov in &self.overrides {
            if ov.start_at.0 <= at.0 && at.0 < ov.end_at.0 {
                return Some(ov.user_id.clone());
            }
        }
        // 2. 否则按 rotation；timezone 与 active_window 判定一起喂给 rotation
        let tz = parse_tz(&self.timezone);
        for rot in &self.rotations {
            if let Some(uid) = rot.resolve_in_tz(at, tz) {
                return Some(uid);
            }
        }
        None
    }
}

/// 解析时区名，失败回落到 UTC。
fn parse_tz(name: &str) -> chrono_tz::Tz {
    if name.is_empty() {
        return chrono_tz::UTC;
    }
    name.parse::<chrono_tz::Tz>().unwrap_or(chrono_tz::UTC)
}

impl Rotation {
    /// 在 `tz` 时区下解析当前 on-call。
    /// - 周期推进按 UTC 时间戳（保证跨时区一致性）；
    /// - 但 `ActiveWindow.{weekday_mask, hour_start, hour_end}` 用 `tz` 本地时间判定。
    pub fn resolve_in_tz(&self, at: TimestampMicros, tz: chrono_tz::Tz) -> Option<Id> {
        if self.members.is_empty() {
            return None;
        }
        let at_dt: DateTime<Utc> = at.to_datetime();
        let start_dt: DateTime<Utc> = self.start_at.to_datetime();
        if at_dt < start_dt {
            return None;
        }
        let period = match self.kind {
            RotationKind::Daily => Duration::days(1),
            RotationKind::Weekly => Duration::weeks(1),
            RotationKind::Custom { period_secs } => Duration::seconds(period_secs as i64),
        };
        let elapsed = at_dt - start_dt;
        let idx = (elapsed.num_seconds() / period.num_seconds().max(1)) as usize;
        let member = &self.members[idx % self.members.len()];

        if let Some(w) = self.active_window
            && !w.contains(at_dt, tz)
        {
            return None;
        }
        Some(member.clone())
    }

    /// 向后兼容的 UTC 版本。
    pub fn resolve(&self, at: TimestampMicros) -> Option<Id> {
        self.resolve_in_tz(at, chrono_tz::UTC)
    }
}

impl ActiveWindow {
    /// 判断 UTC 时间戳 `at_utc` 在 `tz` 本地时区下是否落在窗口内。
    pub fn contains(&self, at_utc: DateTime<Utc>, tz: chrono_tz::Tz) -> bool {
        use chrono::{Datelike, Timelike};
        let local = at_utc.with_timezone(&tz);
        // 约定 weekday_mask bit0=周日, bit6=周六
        let weekday_idx: u8 = match local.weekday() {
            chrono::Weekday::Sun => 0,
            chrono::Weekday::Mon => 1,
            chrono::Weekday::Tue => 2,
            chrono::Weekday::Wed => 3,
            chrono::Weekday::Thu => 4,
            chrono::Weekday::Fri => 5,
            chrono::Weekday::Sat => 6,
        };
        if (self.weekday_mask & (1 << weekday_idx)) == 0 {
            return false;
        }
        let hour = local.hour() as u8;
        if self.hour_start <= self.hour_end {
            self.hour_start <= hour && hour < self.hour_end
        } else {
            // 跨午夜窗口，如 22 → 6
            hour >= self.hour_start || hour < self.hour_end
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn ts(s: &str) -> TimestampMicros {
        let dt = chrono::DateTime::parse_from_rfc3339(s)
            .expect("rfc3339")
            .with_timezone(&Utc);
        TimestampMicros::from_datetime(dt)
    }

    #[test]
    fn active_window_weekday_match_in_shanghai() {
        // 2026-05-23 02:00 UTC == 10:00 CST (Asia/Shanghai), weekday = Sat (bit 6)
        let w = ActiveWindow {
            weekday_mask: 1 << 6,
            hour_start: 9,
            hour_end: 18,
        };
        let at = ts("2026-05-23T02:00:00Z");
        assert!(w.contains(at.to_datetime(), chrono_tz::Asia::Shanghai));
        // 同一时刻在 UTC 评估则不该命中（UTC 02 点）
        assert!(!w.contains(at.to_datetime(), chrono_tz::UTC));
    }

    #[test]
    fn active_window_crosses_midnight() {
        // 22 → 6 跨午夜：本地 23:00 应命中，10:00 不命中
        let w = ActiveWindow {
            weekday_mask: 0xFF,
            hour_start: 22,
            hour_end: 6,
        };
        let utc = chrono_tz::UTC;
        let inside = chrono::Utc.with_ymd_and_hms(2026, 5, 23, 23, 0, 0).unwrap();
        let outside = chrono::Utc.with_ymd_and_hms(2026, 5, 23, 10, 0, 0).unwrap();
        assert!(w.contains(inside, utc));
        assert!(!w.contains(outside, utc));
    }

    #[test]
    fn rotation_picks_member_by_period_then_filters_window() {
        let members: Vec<Id> = (0..3).map(|_| Id::new()).collect();
        let start = ts("2026-05-20T00:00:00Z");
        // weekly rotation, no window → 第 0 天命中 members[0]
        let rot = Rotation {
            id: Id::new(),
            name: "primary".into(),
            members: members.clone(),
            kind: RotationKind::Weekly,
            active_window: None,
            start_at: start,
        };
        let at = ts("2026-05-21T03:00:00Z");
        assert_eq!(rot.resolve(at), Some(members[0].clone()));

        // 一周后 -> members[1]
        let at2 = ts("2026-05-28T00:00:00Z");
        assert_eq!(rot.resolve(at2), Some(members[1].clone()));

        // 加 active_window 只允许工作日 9-18（Mon=2, Tue=4, Wed=8, Thu=16, Fri=32）
        // 2026-05-23 是周六 → 不应命中
        let rot2 = Rotation {
            active_window: Some(ActiveWindow {
                weekday_mask: 0b0111110, // Mon-Fri
                hour_start: 9,
                hour_end: 18,
            }),
            ..rot
        };
        let sat = ts("2026-05-23T12:00:00Z");
        assert_eq!(rot2.resolve(sat), None);
        // 2026-06-01 是周一 12:00 UTC，距 start 12 天 → idx=12/7=1 → members[1]
        let mon_noon = ts("2026-06-01T12:00:00Z");
        assert_eq!(rot2.resolve(mon_noon), Some(members[1].clone()));
    }

    #[test]
    fn paused_schedule_never_resolves_an_on_call_member() {
        let member = Id::new();
        let start = ts("2026-05-20T00:00:00Z");
        let schedule = Schedule {
            id: Id::new(),
            org_id: Id::new(),
            name: "paused".into(),
            description: String::new(),
            team_id: None,
            timezone: "UTC".into(),
            enabled: false,
            rotations: vec![Rotation {
                id: Id::new(),
                name: "primary".into(),
                members: vec![member],
                kind: RotationKind::Daily,
                active_window: None,
                start_at: start,
            }],
            overrides: vec![],
            created_by: None,
            updated_by: None,
            created_at: start,
            updated_at: start,
        };

        assert_eq!(schedule.who_is_on_call(ts("2026-05-21T03:00:00Z")), None);
    }
}
