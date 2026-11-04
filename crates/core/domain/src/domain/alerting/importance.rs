// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 组织的优先级档位与紧急度档位。
//!
//! - **优先级**：有几档（1 到 [`MAX_PRIORITY_LEVELS`]）和每档的名称、说明；键固定是 `P1`…`Pn`。
//! - **紧急度**：内置高、低两档（不能删，能改名），最多再加几档，一共不超过
//!   [`MAX_URGENCY_LEVELS`] 档，最急的在前。每档定四件事：通知完第一级还往不往下升
//!   （[`UrgencyLevel::escalates`]）、免打扰时挡不挡（[`QuietHours`]）、用个人通知设置的哪组步骤
//!   （[`StepGroup`]）、「按严重度」时哪些严重度落到这一档（四个严重度各落一档）。
//!
//! 组织没改过时用内置的：P1–P5；高（升级、看个人开关、高组、critical 和 error），低（不升级、挡、
//! 低组、warning 和 info）。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{
    incident::Severity,
    priority::{MAX_PRIORITY_LEVELS, Priority},
    urgency::Urgency,
};
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

/// 紧急度最多几档。
pub const MAX_URGENCY_LEVELS: usize = 5;

/// 一档的名称最多几个字。
pub const MAX_LEVEL_NAME_CHARS: usize = 32;

/// 一档的说明最多几个字。
pub const MAX_LEVEL_DESCRIPTION_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriorityLevel {
    pub priority: Priority,
    /// 空着时界面按语言显示默认名。
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// 免打扰时段里这一档怎么算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuietHours {
    /// 一律不打扰。
    Block,
    /// 看个人的「免打扰时仍接收高紧急度」开关。
    Switch,
}

impl QuietHours {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Switch => "switch",
        }
    }
}

/// 个人通知设置里的哪组步骤。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepGroup {
    #[default]
    High,
    Low,
}

impl StepGroup {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Low => "low",
        }
    }

    pub fn from_label(text: &str) -> Option<Self> {
        match text.trim() {
            "high" => Some(Self::High),
            "low" => Some(Self::Low),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UrgencyLevel {
    pub key: Urgency,
    /// 空着时界面按语言显示默认名。
    #[serde(default)]
    pub name: String,
    /// 通知完第一级还往下升、整轮重复。
    pub escalates: bool,
    pub quiet_hours: QuietHours,
    pub steps: StepGroup,
    /// 「按严重度」时落到这一档的严重度。
    #[serde(default)]
    pub severities: Vec<Severity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportanceLevels {
    pub priorities: Vec<PriorityLevel>,
    /// 最急的在前。
    pub urgencies: Vec<UrgencyLevel>,
}

impl Default for ImportanceLevels {
    fn default() -> Self {
        Self {
            priorities: (1..=5)
                .filter_map(Priority::of_rank)
                .map(|priority| PriorityLevel {
                    priority,
                    name: String::new(),
                    description: String::new(),
                })
                .collect(),
            urgencies: vec![
                UrgencyLevel {
                    key: Urgency::high(),
                    name: String::new(),
                    escalates: true,
                    quiet_hours: QuietHours::Switch,
                    steps: StepGroup::High,
                    severities: vec![Severity::Critical, Severity::Error],
                },
                UrgencyLevel {
                    key: Urgency::low(),
                    name: String::new(),
                    escalates: false,
                    quiet_hours: QuietHours::Block,
                    steps: StepGroup::Low,
                    severities: vec![Severity::Warning, Severity::Info],
                },
            ],
        }
    }
}

const SEVERITIES: [Severity; 4] = [
    Severity::Critical,
    Severity::Error,
    Severity::Warning,
    Severity::Info,
];

impl ImportanceLevels {
    /// 去掉名称两边的空白，按档排好优先级；检查档数、键、名称和严重度的分配。
    pub fn normalize(&mut self) -> Result<()> {
        self.priorities.sort_by_key(|level| level.priority);

        let count = self.priorities.len();

        if count == 0 || count > usize::from(MAX_PRIORITY_LEVELS) {
            return Err(Error::invalid(format!(
                "priorities have 1 to {MAX_PRIORITY_LEVELS} levels"
            )));
        }

        for (index, level) in self.priorities.iter_mut().enumerate() {
            if usize::from(level.priority.rank()) != index + 1 {
                return Err(Error::invalid(
                    "priority levels run P1, P2, … without a gap",
                ));
            }

            level.name = level.name.trim().to_string();
            level.description = level.description.trim().to_string();
            check_name(&level.name)?;

            if level.description.chars().count() > MAX_LEVEL_DESCRIPTION_CHARS {
                return Err(Error::invalid(format!(
                    "a level description has at most {MAX_LEVEL_DESCRIPTION_CHARS} characters"
                )));
            }
        }

        if self.urgencies.len() < 2 || self.urgencies.len() > MAX_URGENCY_LEVELS {
            return Err(Error::invalid(format!(
                "urgency has 2 to {MAX_URGENCY_LEVELS} levels"
            )));
        }

        for level in &mut self.urgencies {
            level.name = level.name.trim().to_string();
            check_name(&level.name)?;
        }

        let keys: Vec<&Urgency> = self.urgencies.iter().map(|level| &level.key).collect();

        for (index, key) in keys.iter().enumerate() {
            if keys[..index].contains(key) {
                return Err(Error::invalid(format!("urgency `{key}` is listed twice")));
            }
        }

        if !keys.iter().any(|key| key.is_high()) || !keys.iter().any(|key| key.is_low()) {
            return Err(Error::invalid(
                "the built-in urgencies high and low stay; they can be renamed",
            ));
        }

        for severity in SEVERITIES {
            let holders = self
                .urgencies
                .iter()
                .filter(|level| level.severities.contains(&severity))
                .count();

            if holders != 1 {
                return Err(Error::invalid(format!(
                    "severity `{}` goes to exactly one urgency",
                    severity.as_str()
                )));
            }
        }

        Ok(())
    }

    /// 有几档优先级。
    pub fn priority_count(&self) -> u8 {
        u8::try_from(self.priorities.len()).unwrap_or(MAX_PRIORITY_LEVELS)
    }

    /// 写进来的优先级在组织的档里。
    pub fn check_priority(&self, priority: Priority) -> Result<()> {
        let count = self.priority_count();

        if priority.rank() > count {
            return Err(Error::invalid(format!(
                "`{priority}` is not a priority of this organization: use P1 to P{count}"
            )));
        }

        Ok(())
    }

    /// 外部给的优先级超出组织的档数时落到最低一档。
    pub fn fit_priority(&self, priority: Priority) -> Priority {
        Priority::of_rank(priority.rank().min(self.priority_count())).unwrap_or(priority)
    }

    pub fn has_urgency(&self, key: &Urgency) -> bool {
        self.urgencies.iter().any(|level| level.key == *key)
    }

    /// 写进来的紧急度是组织的一档。
    pub fn check_urgency(&self, key: &Urgency) -> Result<()> {
        if !self.has_urgency(key) {
            return Err(Error::invalid(format!(
                "`{key}` is not an urgency of this organization"
            )));
        }

        Ok(())
    }

    /// 这一档怎么通知。档被删了的按高紧急度那档算：宁可多叫一次人，也不漏掉。
    pub fn urgency(&self, key: &Urgency) -> &UrgencyLevel {
        self.urgencies
            .iter()
            .find(|level| level.key == *key)
            .or_else(|| self.urgencies.iter().find(|level| level.key.is_high()))
            .unwrap_or(&self.urgencies[0])
    }

    /// 第几急（0 最急）；没有这一档时按最急算。
    pub fn rank(&self, key: &Urgency) -> usize {
        self.urgencies
            .iter()
            .position(|level| level.key == *key)
            .unwrap_or(0)
    }

    /// 「按严重度」时这个严重度落在哪一档。
    pub fn by_severity(&self, severity: Severity) -> Urgency {
        self.urgencies
            .iter()
            .find(|level| level.severities.contains(&severity))
            .map(|level| level.key.clone())
            .unwrap_or_else(|| {
                if severity >= Severity::Error {
                    Urgency::high()
                } else {
                    Urgency::low()
                }
            })
    }
}

fn check_name(name: &str) -> Result<()> {
    if name.chars().count() > MAX_LEVEL_NAME_CHARS {
        return Err(Error::invalid(format!(
            "a level name has at most {MAX_LEVEL_NAME_CHARS} characters"
        )));
    }

    Ok(())
}

/// 一个组织存下来的档位；没存过的组织用内置的。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredImportance {
    pub levels: ImportanceLevels,
    pub updated_by: Option<Id>,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait ImportanceRepository: Send + Sync {
    async fn get(&self, org_id: &Id) -> Result<Option<StoredImportance>>;
    async fn save(&self, org_id: &Id, stored: &StoredImportance) -> Result<()>;
}

#[cfg(test)]
mod tests;
