// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Organization scoring configuration: evaluators and settings (review threshold, sampling rate,
//! Prompt promotion gate).

use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};

use super::TargetSource;

/// The maximum number of evaluators per organization.
pub const MAX_EVALUATORS_PER_ORG: usize = 10;
pub const MAX_EVALUATOR_NAME_CHARS: usize = 80;
/// The maximum number of characters in a rubric.
pub const MAX_RUBRIC_CHARS: usize = 4_000;
const MAX_MODEL_CHARS: usize = 128;

/// An evaluator that uses a model to score answers against a rubric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evaluator {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    /// The rubric given to the evaluator, inserted into `{{rubric}}` in the `evaluation` Prompt.
    pub rubric: String,
    /// The targets to evaluate; personal conversations are excluded.
    pub targets: Vec<TargetSource>,
    /// The percentage of new targets to evaluate; targets rated unhelpful are always evaluated.
    pub sample_percent: u8,
    /// The model provider to use; if empty, uses the organization's first available provider.
    pub provider_id: Option<Id>,
    /// The model to use; if empty, uses the model provider's default model.
    pub model: Option<String>,
    pub temperature: f32,
    pub enabled: bool,
    /// The language for evaluation reasons: the UI language of the person who last saves it.
    pub language: String,
    pub created_by: Id,
    pub updated_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl Evaluator {
    /// Normalizes and validates: trims names and rubrics, deduplicates targets, and disallows
    /// evaluation of personal conversations.
    pub fn normalized(mut self) -> Result<Self> {
        self.name = self.name.trim().to_string();
        self.rubric = self.rubric.trim().to_string();
        self.model = self
            .model
            .map(|model| model.trim().to_string())
            .filter(|model| !model.is_empty());

        if self.name.is_empty() || self.name.chars().count() > MAX_EVALUATOR_NAME_CHARS {
            return Err(Error::invalid(format!(
                "an evaluator needs a name of at most {MAX_EVALUATOR_NAME_CHARS} characters"
            )));
        }

        if self.rubric.is_empty() || self.rubric.chars().count() > MAX_RUBRIC_CHARS {
            return Err(Error::invalid(format!(
                "an evaluator needs a rubric of at most {MAX_RUBRIC_CHARS} characters"
            )));
        }

        if self
            .model
            .as_ref()
            .is_some_and(|model| model.chars().count() > MAX_MODEL_CHARS)
        {
            return Err(Error::invalid("the model name is too long"));
        }

        self.targets.sort_unstable();
        self.targets.dedup();

        if self.targets.is_empty() {
            return Err(Error::invalid(
                "an evaluator needs at least one kind of answer to score",
            ));
        }

        if self.targets.iter().any(|target| target.is_private()) {
            return Err(Error::invalid(
                "personal chats are only scored by rules: nobody else can read them",
            ));
        }

        if self.sample_percent > 100 {
            return Err(Error::invalid(
                "the sample must be between 0 and 100 percent",
            ));
        }

        if !(0.0..=1.0).contains(&self.temperature) || self.temperature.is_nan() {
            return Err(Error::invalid("the temperature must be between 0 and 1"));
        }

        Ok(self)
    }

    pub fn scores(&self, source: TargetSource) -> bool {
        self.enabled && self.targets.contains(&source)
    }
}

/// How to check an organization Prompt before setting it as the default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateMode {
    /// Skips checks.
    Off,
    /// Sets the default directly if checked with no regression; requires another confirmation if
    /// unchecked or regressed.
    #[default]
    Warn,
    /// Allows setting the default only if checked with no regression.
    Block,
}

impl GateMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Warn => "warn",
            Self::Block => "block",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "off" => Ok(Self::Off),
            "warn" => Ok(Self::Warn),
            "block" => Ok(Self::Block),
            other => Err(Error::invalid(format!("unknown prompt gate `{other}`"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoringSettings {
    /// Scores below this threshold enter pending review (0 to 1).
    pub review_threshold: f64,
    /// The additional percentage of answers randomly selected for review to calibrate evaluators.
    pub review_sample_percent: u8,
    pub gate: GateMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<TimestampMicros>,
}

impl Default for ScoringSettings {
    fn default() -> Self {
        Self {
            review_threshold: 0.6,
            review_sample_percent: 0,
            gate: GateMode::Warn,
            updated_by: None,
            updated_at: None,
        }
    }
}

impl ScoringSettings {
    pub fn validated(self) -> Result<Self> {
        if !(0.0..=1.0).contains(&self.review_threshold) || self.review_threshold.is_nan() {
            return Err(Error::invalid(
                "the review threshold must be between 0 and 1",
            ));
        }

        if self.review_sample_percent > 100 {
            return Err(Error::invalid(
                "the review sample must be between 0 and 100 percent",
            ));
        }

        Ok(self)
    }

    pub fn is_low(&self, score: f64) -> bool {
        score < self.review_threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evaluator() -> Evaluator {
        Evaluator {
            id: Id::from_string("j1"),
            org_id: Id::from_string("org"),
            name: "  Accuracy ".into(),
            rubric: " Is the root cause right? ".into(),
            targets: vec![
                TargetSource::Automation,
                TargetSource::TeamChat,
                TargetSource::TeamChat,
            ],
            sample_percent: 20,
            provider_id: None,
            model: Some("  ".into()),
            temperature: 0.0,
            enabled: true,
            language: "en-us".into(),
            created_by: Id::from_string("u1"),
            updated_by: Id::from_string("u1"),
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        }
    }

    #[test]
    fn an_evaluator_is_trimmed_and_its_targets_deduplicated() {
        let evaluator = evaluator().normalized().unwrap();

        assert_eq!(evaluator.name, "Accuracy");
        assert_eq!(evaluator.rubric, "Is the root cause right?");
        assert_eq!(
            evaluator.targets,
            vec![TargetSource::TeamChat, TargetSource::Automation]
        );
        assert_eq!(evaluator.model, None);
        assert!(evaluator.scores(TargetSource::TeamChat));
        assert!(!evaluator.scores(TargetSource::IncidentRca));
    }

    #[test]
    fn an_evaluator_cannot_read_personal_chats_or_go_out_of_range() {
        let personal = Evaluator {
            targets: vec![TargetSource::PersonalChat],
            ..evaluator()
        };
        assert!(personal.normalized().is_err());

        let none = Evaluator {
            targets: Vec::new(),
            ..evaluator()
        };
        assert!(none.normalized().is_err());

        let hot = Evaluator {
            temperature: 1.5,
            ..evaluator()
        };
        assert!(hot.normalized().is_err());

        let blank = Evaluator {
            rubric: "  ".into(),
            ..evaluator()
        };
        assert!(blank.normalized().is_err());
    }

    #[test]
    fn settings_stay_in_range() {
        assert!(ScoringSettings::default().validated().is_ok());
        assert!(ScoringSettings::default().is_low(0.5));
        assert!(!ScoringSettings::default().is_low(0.75));

        let wrong = ScoringSettings {
            review_threshold: 1.2,
            ..ScoringSettings::default()
        };
        assert!(wrong.validated().is_err());

        for mode in [GateMode::Off, GateMode::Warn, GateMode::Block] {
            assert_eq!(GateMode::parse(mode.as_str()).unwrap(), mode);
        }
    }
}
