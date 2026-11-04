// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Prompt 晋级检查：把一个组织 Prompt 设为默认之前，看新版本有没有比现在的默认差。
//!
//! 样本先取回归集（审核人采纳过的），再取评审最近评过的，最多 [`MAX_CHECK_SAMPLES`] 个。每个样本
//! 用现在的默认与新版本各重新回答一次：工具不再调用，交给模型的是原来那一轮工具查到的结果
//! （事件 RCA 是事件的事实），同一个评审给两份回答打分。新版本的平均分比现在的默认低超过
//! [`MARGIN`] 算变差；评成的样本少于 [`MIN_CHECK_SAMPLES`] 个时说不准。
//!
//! 门禁（[`gate`]）只管 [`CHECKED_PURPOSES`] 里的用途：它们的产出有评审评。

use serde::{Deserialize, Serialize};

use super::{GateMode, ScoreTarget, TargetSource};
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

/// 一次检查最多用这么多个样本。
pub const MAX_CHECK_SAMPLES: usize = 8;
/// 评成的样本少于这么多时说不准。
pub const MIN_CHECK_SAMPLES: usize = 3;
/// 平均分差在这个范围内算一样。
pub const MARGIN: f64 = 0.05;

/// 晋级前要检查的 Prompt 用途，和它们的产出出自哪里。
pub const CHECKED_PURPOSES: [(&str, &[TargetSource]); 2] = [
    (
        "system",
        &[TargetSource::TeamChat, TargetSource::Automation],
    ),
    ("root_cause", &[TargetSource::IncidentRca]),
];

/// 一个用途的产出出自哪里；不在检查范围里时为 `None`。
pub fn checked_sources(purpose: &str) -> Option<&'static [TargetSource]> {
    CHECKED_PURPOSES
        .iter()
        .find(|(checked, _)| *checked == purpose)
        .map(|(_, sources)| *sources)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Queued,
    Running,
    Done,
    Failed,
}

impl CheckStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "done" => Ok(Self::Done),
            "failed" => Ok(Self::Failed),
            other => Err(Error::invalid(format!(
                "unknown prompt check status `{other}`"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckVerdict {
    Better,
    Same,
    Worse,
    /// 评成的样本太少。
    Inconclusive,
}

impl CheckVerdict {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Better => "better",
            Self::Same => "same",
            Self::Worse => "worse",
            Self::Inconclusive => "inconclusive",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "better" => Ok(Self::Better),
            "same" => Ok(Self::Same),
            "worse" => Ok(Self::Worse),
            "inconclusive" => Ok(Self::Inconclusive),
            other => Err(Error::invalid(format!(
                "unknown prompt check verdict `{other}`"
            ))),
        }
    }
}

/// 一个样本的两份回答的分数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckSample {
    pub target_kind: ScoreTarget,
    pub target_id: String,
    /// 来自回归集。
    #[serde(default)]
    pub regression: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate: Option<f64>,
    #[serde(default)]
    pub baseline_reason: String,
    #[serde(default)]
    pub candidate_reason: String,
    /// 这个样本没评成的原因。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl CheckSample {
    pub fn scored(&self) -> Option<(f64, f64)> {
        Some((self.baseline?, self.candidate?))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptCheck {
    pub id: Id,
    pub org_id: Id,
    pub prompt_id: Id,
    pub prompt_version: i32,
    pub purpose: String,
    /// 检查时的默认：组织的默认 Prompt 或内置的。
    pub baseline_prompt_id: Id,
    pub baseline_version: i32,
    pub evaluator_id: Id,
    pub status: CheckStatus,
    #[serde(default)]
    pub samples: Vec<CheckSample>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_mean: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_mean: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<CheckVerdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 评审与重新回答用掉的 token。
    #[serde(default)]
    pub tokens: i64,
    pub requested_by: Id,
    pub created_at: TimestampMicros,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<TimestampMicros>,
}

impl PromptCheck {
    /// 算出两边的平均分与结论。
    pub fn conclude(&mut self) {
        let scored = self
            .samples
            .iter()
            .filter_map(CheckSample::scored)
            .collect::<Vec<_>>();

        if scored.is_empty() {
            self.baseline_mean = None;
            self.candidate_mean = None;
            self.verdict = Some(CheckVerdict::Inconclusive);
            return;
        }

        let count = scored.len() as f64;
        let baseline = scored.iter().map(|(baseline, _)| baseline).sum::<f64>() / count;
        let candidate = scored.iter().map(|(_, candidate)| candidate).sum::<f64>() / count;

        self.baseline_mean = Some(baseline);
        self.candidate_mean = Some(candidate);
        self.verdict = Some(if scored.len() < MIN_CHECK_SAMPLES {
            CheckVerdict::Inconclusive
        } else if candidate < baseline - MARGIN {
            CheckVerdict::Worse
        } else if candidate > baseline + MARGIN {
            CheckVerdict::Better
        } else {
            CheckVerdict::Same
        });
    }

    /// 检查的还是现在这个版本、比的还是现在的默认。
    pub fn current(&self, prompt_version: i32, baseline: (&Id, i32)) -> bool {
        self.prompt_version == prompt_version
            && self.baseline_prompt_id == *baseline.0
            && self.baseline_version == baseline.1
    }
}

/// 为什么不能直接设为默认。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GateReason {
    /// 还没检查这个版本，或检查之后默认变了。
    NotChecked,
    /// 检查还没做完。
    Checking,
    /// 新版本比现在的默认差。
    Worse,
    /// 没有启用的评审评这个用途的产出，检查不了。
    NoEvaluator,
}

impl GateReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotChecked => "prompt_check_required",
            Self::Checking => "prompt_check_running",
            Self::Worse => "prompt_check_worse",
            Self::NoEvaluator => "prompt_check_unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Allow,
    /// 再确认一次就能设。
    Confirm(GateReason),
    Refuse(GateReason),
}

/// 设为默认之前的判定。`check` 是这个 Prompt 最近的检查，`current` 表示它检查的就是现在的
/// 版本与默认；`evaluated` 表示有启用的评审评这个用途的产出；`confirmed` 是设的人已经确认过。
pub fn gate(
    mode: GateMode,
    purpose: &str,
    check: Option<(&PromptCheck, bool)>,
    evaluated: bool,
    confirmed: bool,
) -> Gate {
    if mode == GateMode::Off || checked_sources(purpose).is_none() {
        return Gate::Allow;
    }

    let reason = match check {
        Some((check, true)) => match (check.status, check.verdict) {
            (CheckStatus::Done, Some(CheckVerdict::Worse)) => Some(GateReason::Worse),
            (CheckStatus::Done, _) => None,
            (CheckStatus::Queued | CheckStatus::Running, _) => Some(GateReason::Checking),
            (CheckStatus::Failed, _) if evaluated => Some(GateReason::NotChecked),
            (CheckStatus::Failed, _) => Some(GateReason::NoEvaluator),
        },
        _ if evaluated => Some(GateReason::NotChecked),
        _ => Some(GateReason::NoEvaluator),
    };

    match (reason, mode) {
        (None, _) => Gate::Allow,
        (Some(_), GateMode::Warn) if confirmed => Gate::Allow,
        (Some(reason), GateMode::Warn) => Gate::Confirm(reason),
        (Some(reason), _) => Gate::Refuse(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(baseline: Option<f64>, candidate: Option<f64>) -> CheckSample {
        CheckSample {
            target_kind: ScoreTarget::ChatAnswer,
            target_id: "m1".into(),
            regression: false,
            baseline,
            candidate,
            baseline_reason: String::new(),
            candidate_reason: String::new(),
            error: None,
        }
    }

    fn check(samples: Vec<CheckSample>) -> PromptCheck {
        PromptCheck {
            id: Id::from_string("c1"),
            org_id: Id::from_string("org"),
            prompt_id: Id::from_string("p2"),
            prompt_version: 3,
            purpose: "system".into(),
            baseline_prompt_id: Id::from_string("p1"),
            baseline_version: 1,
            evaluator_id: Id::from_string("e1"),
            status: CheckStatus::Done,
            samples,
            baseline_mean: None,
            candidate_mean: None,
            verdict: None,
            error: None,
            tokens: 0,
            requested_by: Id::from_string("u1"),
            created_at: TimestampMicros(1),
            finished_at: None,
        }
    }

    fn concluded(samples: Vec<CheckSample>) -> PromptCheck {
        let mut check = check(samples);
        check.conclude();
        check
    }

    #[test]
    fn the_verdict_compares_the_means_of_the_scored_samples() {
        let worse = concluded(vec![
            sample(Some(0.75), Some(0.5)),
            sample(Some(1.0), Some(0.75)),
            sample(Some(0.5), Some(0.5)),
            sample(Some(1.0), None),
        ]);
        assert_eq!(worse.verdict, Some(CheckVerdict::Worse));
        assert_eq!(worse.baseline_mean, Some(0.75));

        let same = concluded(vec![
            sample(Some(0.75), Some(0.75)),
            sample(Some(0.5), Some(0.5)),
            sample(Some(1.0), Some(1.0)),
        ]);
        assert_eq!(same.verdict, Some(CheckVerdict::Same));

        let better = concluded(vec![
            sample(Some(0.25), Some(0.75)),
            sample(Some(0.5), Some(0.75)),
            sample(Some(0.5), Some(0.5)),
        ]);
        assert_eq!(better.verdict, Some(CheckVerdict::Better));

        let thin = concluded(vec![sample(Some(1.0), Some(0.0)), sample(None, Some(1.0))]);
        assert_eq!(thin.verdict, Some(CheckVerdict::Inconclusive));
    }

    #[test]
    fn a_check_is_stale_after_an_edit_or_a_new_default() {
        let check = check(Vec::new());

        assert!(check.current(3, (&Id::from_string("p1"), 1)));
        assert!(!check.current(4, (&Id::from_string("p1"), 1)));
        assert!(!check.current(3, (&Id::from_string("p9"), 1)));
    }

    #[test]
    fn the_gate_follows_the_mode_and_the_latest_check() {
        let passed = concluded(vec![
            sample(Some(0.5), Some(0.5)),
            sample(Some(0.5), Some(0.5)),
            sample(Some(0.5), Some(0.5)),
        ]);
        let worse = concluded(vec![
            sample(Some(1.0), Some(0.0)),
            sample(Some(1.0), Some(0.0)),
            sample(Some(1.0), Some(0.0)),
        ]);

        assert_eq!(
            gate(GateMode::Off, "system", None, true, false),
            Gate::Allow
        );
        assert_eq!(
            gate(GateMode::Block, "alert_explain", None, true, false),
            Gate::Allow
        );
        assert_eq!(
            gate(
                GateMode::Block,
                "system",
                Some((&passed, true)),
                true,
                false
            ),
            Gate::Allow
        );
        assert_eq!(
            gate(
                GateMode::Block,
                "system",
                Some((&passed, false)),
                true,
                false
            ),
            Gate::Refuse(GateReason::NotChecked)
        );
        assert_eq!(
            gate(GateMode::Block, "system", Some((&worse, true)), true, true),
            Gate::Refuse(GateReason::Worse)
        );
        assert_eq!(
            gate(GateMode::Warn, "system", Some((&worse, true)), true, false),
            Gate::Confirm(GateReason::Worse)
        );
        assert_eq!(
            gate(GateMode::Warn, "system", Some((&worse, true)), true, true),
            Gate::Allow
        );
        assert_eq!(
            gate(GateMode::Block, "root_cause", None, false, false),
            Gate::Refuse(GateReason::NoEvaluator)
        );
    }

    #[test]
    fn names_round_trip() {
        for status in [
            CheckStatus::Queued,
            CheckStatus::Running,
            CheckStatus::Done,
            CheckStatus::Failed,
        ] {
            assert_eq!(CheckStatus::parse(status.as_str()).unwrap(), status);
        }

        for verdict in [
            CheckVerdict::Better,
            CheckVerdict::Same,
            CheckVerdict::Worse,
            CheckVerdict::Inconclusive,
        ] {
            assert_eq!(CheckVerdict::parse(verdict.as_str()).unwrap(), verdict);
        }

        assert_eq!(
            checked_sources("root_cause"),
            Some(&[TargetSource::IncidentRca][..])
        );
        assert_eq!(checked_sources("evaluation"), None);
    }
}
