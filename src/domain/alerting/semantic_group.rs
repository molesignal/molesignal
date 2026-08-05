// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Semantic groups（告警分组规则，Alertmanager `group_by` 风格）。
//!
//! 一条 [`SemanticGroup`] = 一组 `matchers`（全部命中才适用）+ 一组 `group_by` label
//! 键。incident 触发时，evaluator 取该 org 第一条 enabled 且 matchers 全命中的
//! semantic group，用它的 `group_by` 值派生 grouping key，喂给 incident grouping
//! ——于是不同规则、但 group_by 取值相同的相关 incident 被收拢进同一个
//! [`super::incident_group::IncidentGroup`]。没有任何 group 命中时回退到 incident
//! 自身 fingerprint（即按规则 + 标签分组，原行为）。
//!
//! 匹配逻辑放 domain（纯函数）：app 层 evaluator 创建 incident 时直接调用。

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Result, ids::Id, time::TimestampMicros};

/// 标签匹配算子。`eq`/`neq` 精确比较；`re`/`nre` 整串正则（必须匹配整个值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchOp {
    Eq,
    Neq,
    Re,
    Nre,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelMatcher {
    pub label: String,
    pub op: MatchOp,
    pub value: String,
}

impl LabelMatcher {
    /// 该 matcher 是否命中给定标签集。缺失标签按空串参与比较；**非法正则一律视为该
    /// matcher 不命中**（对 `Re` 与 `Nre` 都 fail-safe——否则坏的 `Nre` 会反转成
    /// 「匹配一切」、把所有 incident 吸进该 group）。
    pub fn matches(&self, labels: &BTreeMap<String, String>) -> bool {
        let actual = labels.get(&self.label).map(String::as_str).unwrap_or("");
        match self.op {
            MatchOp::Eq => actual == self.value,
            MatchOp::Neq => actual != self.value,
            MatchOp::Re | MatchOp::Nre => {
                let Some(is_match) = try_full_match_regex(&self.value, actual) else {
                    // 编译失败：Re / Nre 都返回「不命中」，绝不反转成 match-everything。
                    return false;
                };
                if self.op == MatchOp::Re {
                    is_match
                } else {
                    !is_match
                }
            }
        }
    }
}

/// 整串正则匹配；编译失败返回 None（caller 把它当「该 matcher 不命中」）。
fn try_full_match_regex(pattern: &str, value: &str) -> Option<bool> {
    let anchored = format!("^(?:{pattern})$");
    regex::Regex::new(&anchored)
        .ok()
        .map(|re| re.is_match(value))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticGroup {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub enabled: bool,
    /// 全部命中该 group 才适用。空 matchers = 匹配任何 incident（catch-all）。
    pub matchers: Vec<LabelMatcher>,
    /// 参与分组的 label 键；这些键的取值组合成 grouping fingerprint。
    pub group_by: Vec<String>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl SemanticGroup {
    /// 全部 matcher 命中才算适用。
    pub fn matches(&self, labels: &BTreeMap<String, String>) -> bool {
        self.matchers.iter().all(|m| m.matches(labels))
    }

    /// 由 `group_by` 各键在 incident 标签里的取值派生稳定 grouping key（缺失键取空串）。
    /// `group_by` 为空时回退到 group 名，使该 group 命中的所有 incident 收成一组。
    pub fn group_key(&self, labels: &BTreeMap<String, String>) -> String {
        if self.group_by.is_empty() {
            return format!("group:{}", self.name);
        }
        let mut parts = Vec::with_capacity(self.group_by.len());
        for key in &self.group_by {
            let val = labels.get(key).map(String::as_str).unwrap_or("");
            parts.push(format!("{key}={val}"));
        }
        parts.join(";")
    }
}

#[async_trait]
pub trait SemanticGroupRepository: Send + Sync {
    async fn create(&self, group: SemanticGroup) -> Result<SemanticGroup>;
    async fn update(&self, group: SemanticGroup) -> Result<SemanticGroup>;
    async fn get(&self, id: &Id) -> Result<SemanticGroup>;
    async fn list(&self, org_id: &Id) -> Result<Vec<SemanticGroup>>;
    async fn delete(&self, id: &Id) -> Result<()>;
    /// evaluator 分组时取该 org 所有 enabled 的 group，**按定义顺序（created_at 升序）**
    /// 返回；多条都命中时调用方取第一条，故 precedence = 创建顺序（不是名字）。
    async fn list_enabled(&self, org_id: &Id) -> Result<Vec<SemanticGroup>>;
    /// 批量创建（导入）。默认逐条 create（非原子）；Pg 实装应覆盖为单事务，让
    /// 中途失败整体回滚。
    async fn create_many(&self, groups: Vec<SemanticGroup>) -> Result<usize> {
        let mut n = 0;
        for g in groups {
            self.create(g).await?;
            n += 1;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn group(matchers: Vec<LabelMatcher>, group_by: &[&str]) -> SemanticGroup {
        SemanticGroup {
            id: Id::from_string("g1"),
            org_id: Id::from_string("o1"),
            name: "g".into(),
            enabled: true,
            matchers,
            group_by: group_by.iter().map(|s| s.to_string()).collect(),
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    #[test]
    fn eq_neq_matchers() {
        let g = group(
            vec![LabelMatcher {
                label: "severity".into(),
                op: MatchOp::Eq,
                value: "critical".into(),
            }],
            &["service"],
        );
        assert!(g.matches(&labels(&[("severity", "critical"), ("service", "api")])));
        assert!(!g.matches(&labels(&[("severity", "warning")])));
    }

    #[test]
    fn regex_matcher_is_anchored_and_failsafe() {
        let g = group(
            vec![LabelMatcher {
                label: "service".into(),
                op: MatchOp::Re,
                value: "api-.*".into(),
            }],
            &[],
        );
        assert!(g.matches(&labels(&[("service", "api-gateway")])));
        // 锚定：不能只匹配子串。
        assert!(!g.matches(&labels(&[("service", "legacy-api-x")])));
        // 非法正则 → fail-safe 不命中。
        let bad = group(
            vec![LabelMatcher {
                label: "service".into(),
                op: MatchOp::Re,
                value: "(".into(),
            }],
            &[],
        );
        assert!(!bad.matches(&labels(&[("service", "anything")])));
    }

    #[test]
    fn invalid_nre_regex_does_not_match_everything() {
        // 坏的 Nre 绝不能反转成「匹配一切」。
        let g = group(
            vec![LabelMatcher {
                label: "service".into(),
                op: MatchOp::Nre,
                value: "(".into(),
            }],
            &[],
        );
        assert!(!g.matches(&labels(&[("service", "anything")])));
        // 合法 Nre：值不匹配 pattern → 命中（取反）。
        let ok = group(
            vec![LabelMatcher {
                label: "service".into(),
                op: MatchOp::Nre,
                value: "api-.*".into(),
            }],
            &[],
        );
        assert!(ok.matches(&labels(&[("service", "web")])));
        assert!(!ok.matches(&labels(&[("service", "api-gw")])));
    }

    #[test]
    fn empty_matchers_is_catch_all() {
        let g = group(vec![], &["service"]);
        assert!(g.matches(&labels(&[("anything", "x")])));
    }

    #[test]
    fn group_key_uses_group_by_values() {
        let g = group(vec![], &["service", "region"]);
        assert_eq!(
            g.group_key(&labels(&[("service", "api"), ("region", "us"), ("x", "y")])),
            "service=api;region=us"
        );
        // 缺失键取空串，仍然稳定。
        assert_eq!(
            g.group_key(&labels(&[("service", "api")])),
            "service=api;region="
        );
        // 空 group_by 回退到 group 名。
        let cg = group(vec![], &[]);
        assert_eq!(cg.group_key(&labels(&[])), "group:g");
    }
}
