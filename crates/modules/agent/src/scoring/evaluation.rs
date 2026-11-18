// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Evaluates a target by giving it to a model to score against a rubric.
//!
//! The system message is the organization's `evaluation` Prompt, filled with the rubric and the
//! language for reasons, always followed by [`VERDICT_INSTRUCTION`]: scores remain readable after
//! Prompt changes. The user message contains the material to evaluate: the question, tool
//! findings, and answer; for incident RCA, the incident facts and RCA. Material is truncated by
//! length. The model gives a score from 1 to 5, converted to 0 to 1.

use kernel::{Error, Result};
use serde_json::Value;

use super::ScoreTarget;
use crate::runs::TranscriptStep;

/// The evaluation Prompt's purpose (matches the built-in `evaluation.default` in baseline).
pub const EVALUATION_PURPOSE: &str = "evaluation";

/// The output format appended to the evaluation Prompt.
pub const VERDICT_INSTRUCTION: &str = "Return only one JSON object without a Markdown fence: \
     {\"score\": <an integer from 1 (unusable) to 5 (excellent)>, \"reason\": \"<at most two \
     sentences on the main strength or flaw>\"}.";

const QUESTION_CHARS: usize = 4_000;
const ANSWER_CHARS: usize = 8_000;
const NOTE_CHARS: usize = 2_000;
/// The maximum characters given to the evaluator for one tool result and for all tool results
/// combined.
const RESULT_CHARS: usize = 1_500;
const FINDINGS_CHARS: usize = 12_000;
const ARGUMENT_CHARS: usize = 400;
const REASON_CHARS: usize = 500;

/// The material to evaluate.
#[derive(Debug, Clone, Copy)]
pub struct Material<'a> {
    pub kind: ScoreTarget,
    /// The question for a conversation answer; the incident facts for incident RCA.
    pub question: &'a str,
    /// Tool calls and results during answer generation.
    pub findings: &'a [TranscriptStep],
    pub answer: &'a str,
    /// The reviewer's note on the original answer, present only for targets in the regression set.
    pub note: Option<&'a str>,
}

/// The user message given to the evaluator.
pub fn material_text(material: &Material<'_>) -> String {
    let (question_heading, answer_heading) = match material.kind {
        ScoreTarget::ChatAnswer => ("Question", "Answer"),
        ScoreTarget::IncidentRca => ("Incident", "Root cause analysis"),
    };

    let mut text = format!(
        "## {question_heading}\n\n{}\n\n",
        clip(material.question, QUESTION_CHARS)
    );

    let findings = findings_text(material.findings);

    if !findings.is_empty() {
        text.push_str("## What the tools found\n\n");
        text.push_str(&findings);
        text.push_str("\n\n");
    }

    text.push_str(&format!(
        "## {answer_heading}\n\n{}",
        clip(material.answer, ANSWER_CHARS)
    ));

    if let Some(note) = material.note.map(str::trim).filter(|note| !note.is_empty()) {
        text.push_str(&format!(
            "\n\n## A reviewer's note on an earlier answer to this question\n\n{}",
            clip(note, NOTE_CHARS)
        ));
    }

    text
}

/// Lists one tool call per line, with truncated results indented below.
/// Also given to the model in this form when regenerating answers for promotion checks.
pub fn findings_text(steps: &[TranscriptStep]) -> String {
    let mut text = String::new();
    let mut index = 0;

    for call in steps.iter().flat_map(|step| step.calls.iter()) {
        index += 1;

        let arguments = clip(&call.arguments.to_string(), ARGUMENT_CHARS);
        let outcome = if call.is_error { " (failed)" } else { "" };
        let result = call
            .result
            .as_deref()
            .map(|result| clip(result, RESULT_CHARS))
            .unwrap_or_else(|| "[no result]".into());

        let entry = format!(
            "{index}. {} {arguments}{outcome}\n   {}\n",
            call.name,
            result.replace('\n', "\n   ")
        );

        if text.len() + entry.len() > FINDINGS_CHARS {
            text.push_str("[more tool calls were left out]\n");
            break;
        }

        text.push_str(&entry);
    }

    text.trim_end().to_string()
}

/// The name of the language for reasons, inserted into `{{output_language}}` in the evaluation
/// Prompt.
pub fn output_language(language: &str) -> &'static str {
    if language.trim().to_ascii_lowercase().starts_with("zh") {
        "Simplified Chinese"
    } else {
        "English"
    }
}

/// The score given by the evaluator.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// 0 to 1.
    pub score: f64,
    /// The model's score from 1 to 5.
    pub points: u8,
    pub reason: String,
}

/// Reads the evaluator's response: a JSON object, optionally fenced or surrounded by text.
pub fn parse_verdict(content: &str) -> Result<Verdict> {
    let object = json_object(content)
        .ok_or_else(|| Error::internal("the evaluator did not return a JSON object"))?;

    let points = object
        .get("score")
        .and_then(|score| {
            score
                .as_f64()
                .or_else(|| score.as_str().and_then(|text| text.trim().parse().ok()))
        })
        .filter(|score| score.fract() == 0.0 && (1.0..=5.0).contains(score))
        .ok_or_else(|| Error::internal("the evaluator's score is not an integer from 1 to 5"))?
        as u8;

    let reason = object
        .get("reason")
        .and_then(Value::as_str)
        .map(|reason| clip(reason.trim(), REASON_CHARS))
        .unwrap_or_default();

    Ok(Verdict {
        score: f64::from(points - 1) / 4.0,
        points,
        reason,
    })
}

fn json_object(content: &str) -> Option<serde_json::Map<String, Value>> {
    let start = content.find('{')?;
    let end = content.rfind('}')?;

    if end < start {
        return None;
    }

    match serde_json::from_str::<Value>(&content[start..=end]).ok()? {
        Value::Object(object) => Some(object),
        _ => None,
    }
}

/// Limits text to `chars` characters, with a note at the truncation point.
fn clip(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars {
        return text.to_string();
    }

    let mut cut = text.chars().take(chars).collect::<String>();
    cut.push_str(" […]");
    cut
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::runs::TranscriptCall;

    #[test]
    fn reads_the_score_from_a_fenced_or_wordy_reply() {
        let verdict =
            parse_verdict("```json\n{\"score\": 4, \"reason\": \" Right cause. \"}\n```").unwrap();
        assert_eq!(verdict.points, 4);
        assert_eq!(verdict.score, 0.75);
        assert_eq!(verdict.reason, "Right cause.");

        let wordy = parse_verdict("Here you go: {\"score\": \"2\"} hope it helps").unwrap();
        assert_eq!(wordy.score, 0.25);
        assert_eq!(wordy.reason, "");
    }

    #[test]
    fn a_score_outside_one_to_five_or_without_json_is_an_error() {
        assert!(parse_verdict("{\"score\": 7}").is_err());
        assert!(parse_verdict("{\"score\": 3.5}").is_err());
        assert!(parse_verdict("four out of five").is_err());
        assert!(parse_verdict("[1, 2]").is_err());
    }

    #[test]
    fn material_carries_the_question_findings_answer_and_note() {
        let steps = vec![TranscriptStep {
            text: String::new(),
            calls: vec![TranscriptCall {
                id: "c1".into(),
                name: "query_logs".into(),
                arguments: json!({ "sql": "SELECT count(*) FROM nginx" }),
                result: Some("{\"rows\":[[42]]}".into()),
                truncated: false,
                is_error: true,
            }],
        }];

        let text = material_text(&Material {
            kind: ScoreTarget::ChatAnswer,
            question: "Why is checkout slow?",
            findings: &steps,
            answer: "The database is saturated.",
            note: Some("It was the cache, not the database."),
        });

        assert!(text.starts_with("## Question\n\nWhy is checkout slow?"));
        assert!(text.contains("1. query_logs {\"sql\":\"SELECT count(*) FROM nginx\"} (failed)"));
        assert!(text.contains("## Answer\n\nThe database is saturated."));
        assert!(text.ends_with("It was the cache, not the database."));

        let rca = material_text(&Material {
            kind: ScoreTarget::IncidentRca,
            question: "Incident: High error rate",
            findings: &[],
            answer: "A bad deploy.",
            note: None,
        });

        assert_eq!(
            rca,
            "## Incident\n\nIncident: High error rate\n\n## Root cause analysis\n\nA bad deploy."
        );
    }

    #[test]
    fn long_material_is_clipped() {
        let long = "x".repeat(QUESTION_CHARS + 10);
        let text = material_text(&Material {
            kind: ScoreTarget::ChatAnswer,
            question: &long,
            findings: &[],
            answer: "ok",
            note: None,
        });

        assert!(text.contains(" […]"));
        assert_eq!(output_language("zh-cn"), "Simplified Chinese");
        assert_eq!(output_language("en-us"), "English");
    }
}
