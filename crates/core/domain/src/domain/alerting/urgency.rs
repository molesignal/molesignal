// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 紧急度：现在叫不叫醒人。紧急度是一档的键：内置高（`high`）、低（`low`），组织还能再加几档；
//! 每档怎么通知（往不往下升、免打扰时挡不挡、用个人的哪组步骤）见 [`super::importance`]。
//!
//! - 内置的高：按个人的高紧急度通知步骤通知，照升级策略升级；免打扰时段按个人的开关决定。
//! - 内置的低：按个人的低紧急度步骤通知（没设时只用高紧急度的第一步），只通知升级策略的第一级、
//!   不往下升（升级策略可以改成照常升级），免打扰时段一律不打扰。
//!
//! 开事件时定下：规则、外部告警、告警流水线或手动建事件明确指定的优先；否则看事件走的升级策略的
//! [`UrgencyRule`]，默认按严重度（组织的档位里每个严重度落在哪一档；内置是 critical、error 高，
//! warning、info 低）。

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{
    importance::ImportanceLevels,
    incident::Severity,
    time_window::{CalendarUse, WeeklyWindows},
};
use crate::shared::{Error, Result, time::TimestampMicros};

/// 档的键最长几个字符。
pub const MAX_URGENCY_KEY_CHARS: usize = 32;

/// 一档紧急度的键：小写字母开头，小写字母、数字和下划线。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Urgency(String);

impl Urgency {
    pub const HIGH: &'static str = "high";
    pub const LOW: &'static str = "low";

    pub fn high() -> Self {
        Self(Self::HIGH.into())
    }

    pub fn low() -> Self {
        Self(Self::LOW.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_high(&self) -> bool {
        self.0 == Self::HIGH
    }

    pub fn is_low(&self) -> bool {
        self.0 == Self::LOW
    }

    /// 读一个键，两边的空白去掉、大写换成小写；不是合法的键时为空。
    pub fn from_label(text: &str) -> Option<Self> {
        let key = text.trim().to_ascii_lowercase();
        let valid = key.len() <= MAX_URGENCY_KEY_CHARS
            && key.starts_with(|c: char| c.is_ascii_lowercase())
            && key
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');

        valid.then_some(Self(key))
    }

    pub fn parse(text: &str) -> Result<Self> {
        Self::from_label(text).ok_or_else(|| {
            Error::invalid(format!(
                "`{text}` is not an urgency: use a lowercase key such as high or low"
            ))
        })
    }
}

impl Default for Urgency {
    fn default() -> Self {
        Self::high()
    }
}

impl fmt::Display for Urgency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for Urgency {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Urgency {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

fn high() -> Urgency {
    Urgency::high()
}

fn low() -> Urgency {
    Urgency::low()
}

/// 升级策略怎么定紧急度（事件没有明确指定时）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "mode",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum UrgencyRule {
    /// 每个严重度落在组织的哪一档（内置：critical、error 高，warning、info 低）。
    #[default]
    BySeverity,
    /// 总是这一档。
    Always { urgency: Urgency },
    /// 支持时段内用 `inside`（默认高）、时段外用 `outside`（默认低）；`raise_at_start` 时，支持
    /// 时段开始那一刻还没确认、比 `inside` 不急的事件升到 `inside`，从第一级重新通知。
    SupportHours {
        hours: WeeklyWindows,
        #[serde(default)]
        raise_at_start: bool,
        #[serde(default = "high")]
        inside: Urgency,
        #[serde(default = "low")]
        outside: Urgency,
    },
}

impl UrgencyRule {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::SupportHours { hours, .. } => hours.validate(),
            _ => Ok(()),
        }
    }

    /// 用到的档都在组织的档位里。
    pub fn check_levels(&self, levels: &ImportanceLevels) -> Result<()> {
        match self {
            Self::BySeverity => Ok(()),
            Self::Always { urgency } => levels.check_urgency(urgency),
            Self::SupportHours {
                inside, outside, ..
            } => {
                levels.check_urgency(inside)?;
                levels.check_urgency(outside)
            }
        }
    }

    /// 支持时段引用的节假日日历。
    pub fn calendar(&self) -> Option<&CalendarUse> {
        match self {
            Self::SupportHours { hours, .. } => hours.calendar.as_ref(),
            _ => None,
        }
    }

    /// 在 `at` 开的、严重度为 `severity` 的事件的紧急度。
    pub fn urgency(
        &self,
        severity: Severity,
        at: TimestampMicros,
        levels: &ImportanceLevels,
    ) -> Urgency {
        match self {
            Self::BySeverity => levels.by_severity(severity),
            Self::Always { urgency } => urgency.clone(),
            Self::SupportHours {
                hours,
                inside,
                outside,
                ..
            } => {
                if hours.contains(at) {
                    inside.clone()
                } else {
                    outside.clone()
                }
            }
        }
    }

    /// 支持时段开始时把事件升到哪一档：只有要在那时升级的支持时段规则有。
    pub fn raises_to(&self) -> Option<&Urgency> {
        match self {
            Self::SupportHours {
                raise_at_start: true,
                inside,
                ..
            } => Some(inside),
            _ => None,
        }
    }

    /// 支持时段在 `(since, now]` 里开始过，且这个规则要在那时把事件升上去：返回开始的时刻。
    pub fn support_hours_began(
        &self,
        since: TimestampMicros,
        now: TimestampMicros,
    ) -> Option<TimestampMicros> {
        let Self::SupportHours {
            hours,
            raise_at_start: true,
            ..
        } = self
        else {
            return None;
        };

        // transitions 列出的是开和关，开的那一刻之后马上在时段里。
        hours
            .transitions(since, TimestampMicros(now.0.saturating_add(1)))
            .into_iter()
            .rev()
            .find(|at| hours.contains(TimestampMicros(*at)))
            .map(TimestampMicros)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::alerting::time_window::{ClockTime, WeeklyRange};

    fn at(text: &str) -> TimestampMicros {
        TimestampMicros::from_datetime(
            chrono::DateTime::parse_from_rfc3339(text)
                .unwrap()
                .with_timezone(&chrono::Utc),
        )
    }

    fn office(raise_at_start: bool) -> UrgencyRule {
        UrgencyRule::SupportHours {
            hours: WeeklyWindows {
                timezone: "UTC".into(),
                ranges: vec![WeeklyRange {
                    weekday_mask: 0b011_1110,
                    start: ClockTime::parse("09:00").unwrap(),
                    end: ClockTime::parse("18:00").unwrap(),
                }],
                calendar: None,
            },
            raise_at_start,
            inside: Urgency::high(),
            outside: Urgency::low(),
        }
    }

    #[test]
    fn urgency_follows_the_severity_unless_the_policy_says_otherwise() {
        let now = at("2026-05-18T10:00:00Z");
        let levels = ImportanceLevels::default();
        let always = |key: &str| UrgencyRule::Always {
            urgency: Urgency::parse(key).unwrap(),
        };

        assert_eq!(
            UrgencyRule::BySeverity.urgency(Severity::Critical, now, &levels),
            Urgency::high()
        );
        assert_eq!(
            UrgencyRule::BySeverity.urgency(Severity::Error, now, &levels),
            Urgency::high()
        );
        assert_eq!(
            UrgencyRule::BySeverity.urgency(Severity::Warning, now, &levels),
            Urgency::low()
        );
        assert_eq!(
            always("high").urgency(Severity::Info, now, &levels),
            Urgency::high()
        );
        assert_eq!(
            always("low").urgency(Severity::Critical, now, &levels),
            Urgency::low()
        );

        // Monday 2026-05-18: 10:00 is inside the hours, 20:00 outside.
        assert_eq!(
            office(false).urgency(Severity::Info, now, &levels),
            Urgency::high()
        );
        assert_eq!(
            office(false).urgency(Severity::Critical, at("2026-05-18T20:00:00Z"), &levels),
            Urgency::low()
        );
    }

    #[test]
    fn support_hours_say_when_they_began_only_when_asked_to_raise() {
        let since = at("2026-05-18T08:59:00Z");
        let now = at("2026-05-18T09:00:30Z");

        assert_eq!(
            office(true).support_hours_began(since, now),
            Some(at("2026-05-18T09:00:00Z"))
        );
        assert_eq!(office(true).raises_to(), Some(&Urgency::high()));
        assert_eq!(office(false).support_hours_began(since, now), None);
        assert_eq!(office(false).raises_to(), None);
        assert_eq!(
            office(true).support_hours_began(now, at("2026-05-18T12:00:00Z")),
            None
        );

        let json = serde_json::to_value(office(true)).unwrap();
        assert_eq!(json["mode"], "support_hours");
        assert_eq!(json["hours"]["ranges"][0]["start"], "09:00");
        assert_eq!(json["inside"], "high");
        assert_eq!(
            serde_json::from_str::<UrgencyRule>(r#"{"mode":"always","urgency":"low"}"#).unwrap(),
            UrgencyRule::Always {
                urgency: Urgency::low()
            }
        );
        assert!(Urgency::from_label("Medium_1").is_some());
        assert!(Urgency::from_label("1st").is_none());
        assert!(Urgency::from_label("night shift").is_none());
    }
}
