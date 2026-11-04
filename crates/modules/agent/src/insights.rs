// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/insight`：回头看一段时间里 Mole Agent 用得怎么样。
//!
//! 范围随会话：团队会话看本队的会话与自动化；个人会话看本人的个人会话、本人所在团队的会话，
//! 以及本人与这些团队的自动化。组织作用域的自动化两种会话都算。只读，不改任何东西。

use std::collections::BTreeMap;

use serde::Serialize;

use crate::shared::{ids::Id, time::TimestampMicros};

/// 回看最多这么多天。
pub const MAX_DAYS: i64 = 90;
pub const DEFAULT_DAYS: i64 = 30;

/// 看谁的：组织里一个人的个人会话（团队会话时为空），加上这些团队。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsightScope {
    pub org_id: Id,
    pub owner_id: Option<Id>,
    pub team_ids: Vec<Id>,
}

/// 一个名字被用了几次。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Count {
    pub name: String,
    pub count: i64,
}

/// 一个工具被调了几次、几次出错。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolUse {
    pub name: String,
    pub calls: i64,
    pub errors: i64,
}

/// 一条自动化的运行，按状态数。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AutomationUse {
    pub name: String,
    pub runs: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Insights {
    pub since: TimestampMicros,
    /// 有人提问的会话数与问题数，自动化的不算。
    pub chats: i64,
    pub questions: i64,
    /// 回答（Run）按状态数。
    pub runs: BTreeMap<String, i64>,
    pub helpful: i64,
    pub unhelpful: i64,
    /// 「没帮助」的原因。
    pub unhelpful_reasons: Vec<Count>,
    /// 调用最多的工具，最多 20 个。
    pub tools: Vec<ToolUse>,
    /// 用到的 Skill：用命令调的，加上模型自己读的。
    pub skills: Vec<Count>,
    pub automations: Vec<AutomationUse>,
}

/// 把一组 (名字, 次数) 合并，多的在前，同样多的按名字排。
pub fn ranked(counts: impl IntoIterator<Item = (String, i64)>) -> Vec<Count> {
    let mut merged = BTreeMap::<String, i64>::new();

    for (name, count) in counts {
        if !name.trim().is_empty() {
            *merged.entry(name).or_default() += count;
        }
    }

    let mut ranked = merged
        .into_iter()
        .map(|(name, count)| Count { name, count })
        .collect::<Vec<_>>();

    ranked.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_merged_and_ranked() {
        let ranked = ranked([
            ("incident-triage".to_string(), 2),
            ("init".to_string(), 3),
            ("incident-triage".to_string(), 2),
            (" ".to_string(), 9),
        ]);

        assert_eq!(
            ranked,
            vec![
                Count {
                    name: "incident-triage".into(),
                    count: 4
                },
                Count {
                    name: "init".into(),
                    count: 3
                },
            ]
        );
    }
}
