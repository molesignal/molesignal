// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 像一个事件的过去的事件：同指纹、同规则、共有服务、标签重合、标题相近、同团队各加分，分高的在前。
//!
//! 标题的相近程度照 `pg_trgm` 的做法：小写后按非字母数字切词，每个词前补两个空格、后补一个空格，取
//! 全部三字串，两边三字串的交集除以并集。汉字算字母数字，中文标题一样能比。

use std::collections::HashSet;

use serde::Serialize;

use crate::{
    domain::alerting::incident::{Incident, IncidentStatus, Severity},
    shared::{ids::Id, time::TimestampMicros},
};

/// 往回找多久。
pub const LOOKBACK_MICROS: i64 = 90 * 86_400_000_000;
/// 最多列几个。
pub const MOST_SIMILAR: usize = 5;
/// 分数到这么高才算像。
pub const MIN_SCORE: f64 = 2.0;

/// 一个像它的事件。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimilarIncident {
    pub id: Id,
    pub summary: String,
    pub severity: Severity,
    pub status: IncidentStatus,
    pub created_at: TimestampMicros,
    pub resolved_at: Option<TimestampMicros>,
    pub resolved_by: Option<Id>,
    pub score: f64,
    /// 哪几处像：`fingerprint`、`rule`、`services`、`labels`、`title`、`team`。
    pub reasons: Vec<&'static str>,
    pub notes: u32,
    /// 根因分析的第一句。
    pub root_cause: Option<String>,
}

/// `candidate` 有多像 `incident`，以及像在哪里。
pub fn score(incident: &Incident, candidate: &Incident) -> (f64, Vec<&'static str>) {
    let mut score = 0.0;
    let mut reasons = Vec::new();

    if incident.fingerprint == candidate.fingerprint {
        score += 5.0;
        reasons.push("fingerprint");
    } else if incident.rule_id == candidate.rule_id {
        score += 3.0;
        reasons.push("rule");
    }

    let services = jaccard(
        incident
            .affected_services
            .iter()
            .map(String::as_str)
            .collect(),
        candidate
            .affected_services
            .iter()
            .map(String::as_str)
            .collect(),
    );

    if services > 0.0 {
        score += 2.0 * services;
        reasons.push("services");
    }

    let pairs = |labels: &std::collections::BTreeMap<String, String>| -> HashSet<String> {
        labels
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect()
    };
    let labels = jaccard_owned(pairs(&incident.labels), pairs(&candidate.labels));

    if labels >= 0.3 {
        score += 2.0 * labels;
        reasons.push("labels");
    }

    let title = trigram_similarity(&incident.summary, &candidate.summary);

    if title >= 0.3 {
        score += 2.0 * title;
        reasons.push("title");
    }

    if incident.team_id.is_some() && incident.team_id == candidate.team_id {
        score += 1.0;
        reasons.push("team");
    }

    (score, reasons)
}

/// 两段文字的三字串相似度，0..1。
pub fn trigram_similarity(a: &str, b: &str) -> f64 {
    jaccard_owned(trigrams(a), trigrams(b))
}

fn trigrams(text: &str) -> HashSet<String> {
    let lower = text.to_lowercase();
    let mut set = HashSet::new();

    for word in lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
    {
        let padded: Vec<char> = format!("  {word} ").chars().collect();

        for window in padded.windows(3) {
            set.insert(window.iter().collect());
        }
    }

    set
}

fn jaccard(a: HashSet<&str>, b: HashSet<&str>) -> f64 {
    let union = a.union(&b).count();

    if union == 0 {
        0.0
    } else {
        a.intersection(&b).count() as f64 / union as f64
    }
}

fn jaccard_owned(a: HashSet<String>, b: HashSet<String>) -> f64 {
    jaccard(
        a.iter().map(String::as_str).collect(),
        b.iter().map(String::as_str).collect(),
    )
}

/// 根因分析正文里第一句说事的话：跳过标题和空行，去掉列表符号，最长 200 字。
pub fn first_sentence(summary: &str) -> Option<String> {
    let line = summary
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))?
        .trim_start_matches(['-', '*', ' '])
        .trim();

    if line.is_empty() {
        return None;
    }

    Some(line.chars().take(200).collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn incident(id: &str, fields: serde_json::Value) -> Incident {
        let mut base = json!({
            "id": id,
            "orgId": "org-a",
            "ruleId": "rule-1",
            "escalationPolicyId": "policy-1",
            "status": "resolved",
            "severity": "critical",
            "summary": "Checkout error rate above 5%",
            "fingerprint": "fp-1",
            "currentStep": 0,
            "currentLoop": 0,
            "currentStepStartedAt": 0,
            "assignees": [],
            "labels": {"service": "checkout", "env": "prod"},
            "annotations": {},
            "traceIds": [],
            "hostIds": [],
            "affectedServices": ["checkout"],
            "triggeringQuery": null,
            "createdAt": 0,
        });

        for (key, value) in fields.as_object().expect("object") {
            base[key] = value.clone();
        }

        serde_json::from_value(base).expect("incident")
    }

    #[test]
    fn the_same_fingerprint_counts_most_and_a_stranger_scores_nothing() {
        let now = incident("now", json!({"teamId": "team-pay"}));

        let (same, reasons) = score(&now, &incident("old", json!({"teamId": "team-pay"})));
        assert_eq!(
            reasons,
            ["fingerprint", "services", "labels", "title", "team"]
        );
        assert!((same - 12.0).abs() < 1e-9);

        let (stranger, reasons) = score(
            &now,
            &incident(
                "other",
                json!({
                    "ruleId": "rule-9",
                    "fingerprint": "fp-9",
                    "summary": "Disk almost full",
                    "labels": {"host": "db-1"},
                    "affectedServices": ["postgres"],
                }),
            ),
        );
        assert_eq!(stranger, 0.0);
        assert!(reasons.is_empty());
    }

    #[test]
    fn titles_compare_by_their_trigrams_in_any_script() {
        assert!(trigram_similarity("Checkout p95 above 800 ms", "Checkout p95 above 900 ms") > 0.6);
        assert!(trigram_similarity("订单服务错误率升高", "订单服务错误率持续升高") > 0.3);
        assert_eq!(trigram_similarity("", "anything"), 0.0);
    }

    #[test]
    fn the_first_sentence_skips_headings_and_bullets() {
        assert_eq!(
            first_sentence(
                "## Likely cause\n\n- The edge proxy rollout dropped keep-alives.\n- More"
            ),
            Some("The edge proxy rollout dropped keep-alives.".into())
        );
        assert_eq!(first_sentence("## Only a heading"), None);
    }
}
