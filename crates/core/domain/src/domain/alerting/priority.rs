// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 优先级：业务上先处理哪个，P1 最重，可以不设。和严重度（信号有多糟）、紧急度（现在叫不叫醒人）
//! 各管一件事；不从严重度推导。来自规则、外部告警、告警流水线或手动设置。
//!
//! 档位的键固定是 `P1`…`Pn`：组织定有几档（1 到 [`MAX_PRIORITY_LEVELS`]）和每档的名称（见
//! [`super::importance`]），所以接口、集成和导入的文件都不受改名影响。

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::shared::{Error, Result};

/// 优先级最多几档。
pub const MAX_PRIORITY_LEVELS: u8 = 9;

/// 第几档：1 最重。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Priority(u8);

impl Priority {
    pub const P1: Self = Self(1);
    pub const P2: Self = Self(2);
    pub const P3: Self = Self(3);
    pub const P4: Self = Self(4);
    pub const P5: Self = Self(5);

    /// 第 `rank` 档，1 到 [`MAX_PRIORITY_LEVELS`]。
    pub fn of_rank(rank: u8) -> Option<Self> {
        (1..=MAX_PRIORITY_LEVELS)
            .contains(&rank)
            .then_some(Self(rank))
    }

    pub const fn rank(self) -> u8 {
        self.0
    }

    pub fn as_str(self) -> String {
        format!("P{}", self.0)
    }

    /// `P1`…`P9`，大小写都认，也认 `1`…`9`。
    pub fn from_label(text: &str) -> Option<Self> {
        let text = text.trim();
        let digits = text.strip_prefix(['P', 'p']).unwrap_or(text);

        if digits.len() != 1 {
            return None;
        }

        digits.parse::<u8>().ok().and_then(Self::of_rank)
    }

    pub fn parse(text: &str) -> Result<Self> {
        Self::from_label(text).ok_or_else(|| {
            Error::invalid(format!(
                "`{text}` is not a priority: use P1 to P{MAX_PRIORITY_LEVELS}"
            ))
        })
    }

    /// 至少和 `bar` 一样重（P1 比 P2 重）。
    pub fn at_least(self, bar: Self) -> bool {
        self <= bar
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "P{}", self.0)
    }
}

impl Serialize for Priority {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Priority {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priorities_read_their_usual_spellings_and_order_from_p1() {
        assert_eq!(Priority::from_label("p2"), Some(Priority::P2));
        assert_eq!(Priority::from_label(" P5 "), Some(Priority::P5));
        assert_eq!(Priority::from_label("3"), Some(Priority::P3));
        assert_eq!(Priority::from_label("P9").map(Priority::rank), Some(9));
        assert_eq!(Priority::from_label("P0"), None);
        assert_eq!(Priority::from_label("P10"), None);
        assert!(Priority::P1.at_least(Priority::P2));
        assert!(!Priority::P3.at_least(Priority::P2));
        assert_eq!(serde_json::to_string(&Priority::P4).unwrap(), "\"P4\"");
        assert!(serde_json::from_str::<Priority>("\"P0\"").is_err());
    }
}
