// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Prompt promotion check: before making an organization Prompt the default, check whether the
//! new version performs worse than the current default.
//!
//! Take samples from the regression set (accepted by reviewers) first, then those recently scored
//! by evaluators, up to [`MAX_CHECK_SAMPLES`]. Generate a new answer for each sample using both the
//! current default and the new version: do not call tools again; give the model the tool results
//! from the original turn (incident facts for incident RCA). The same evaluator scores both
//! answers. The new version is worse if its average score is lower than the current default by
//! more than [`MARGIN`]; the result is inconclusive if fewer than [`MIN_CHECK_SAMPLES`] samples
//! are successfully evaluated.
//!
//! The gate ([`gate`]) only covers purposes in [`CHECKED_PURPOSES`]: their outputs have evaluators.

use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};

use super::{GateMode, ScoreTarget, TargetSource};

/// Use at most this many samples per check.
pub const MAX_CHECK_SAMPLES: usize = 8;
/// The result is inconclusive if fewer than this many samples are successfully evaluated.
pub const MIN_CHECK_SAMPLES: usize = 3;
/// Treat average scores as equal when their difference is within this range.
pub const MARGIN: f64 = 0.05;

/// Prompt purposes to check before promotion, and the sources of their outputs.
pub const CHECKED_PURPOSES: [(&str, &[TargetSource]); 2] = [
    (
        "system",
        &[TargetSource::TeamChat, TargetSource::Automation],
    ),
    ("root_cause", &[TargetSource::IncidentRca]),
];

/// The sources of a purpose's outputs; `None` if the purpose is outside the scope of checks.
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
    /// Too few samples are successfully evaluated.
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

/// Scores for the two answers to one sample.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckSample {
    pub target_kind: ScoreTarget,
    pub target_id: String,
    /// Comes from the regression set.
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
    /// Why this sample is not successfully evaluated.
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
    /// The default at check time: the organization's default Prompt or the built-in one.
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
    /// Tokens used for evaluation and regenerating answers.
    #[serde(default)]
    pub tokens: i64,
    pub requested_by: Id,
    pub created_at: TimestampMicros,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<TimestampMicros>,
}

impl PromptCheck {
    /// Calculate both average scores and the conclusion.
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

    /// The check still covers the current version and compares it against the current default.
    pub fn current(&self, prompt_version: i32, baseline: (&Id, i32)) -> bool {
        self.prompt_version == prompt_version
            && self.baseline_prompt_id == *baseline.0
            && self.baseline_version == baseline.1
    }
}

/// Why it cannot be set as the default directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GateReason {
    /// This version is not checked yet, or the default changes after the check.
    NotChecked,
    /// The check is not complete yet.
    Checking,
    /// The new version performs worse than the current default.
    Worse,
    /// No enabled evaluator scores outputs for this purpose, so the check cannot run.
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
    /// Can be set after another confirmation.
    Confirm(GateReason),
    Refuse(GateReason),
}

/// Decide whether to allow setting the default.
/// `check` is this Prompt's latest check; `current` means it checks the current version against the
/// current default; `evaluated` means an enabled evaluator scores outputs for this purpose;
/// `confirmed` means the person setting it has already confirmed.
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
