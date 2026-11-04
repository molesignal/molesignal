// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 评审：把一个对象交给模型，照评分标准打分。
//!
//! system 是组织的 `evaluation` Prompt，填进评分标准与写理由用的语言，后面固定接上
//! [`VERDICT_INSTRUCTION`]：改了 Prompt 也读得出分数。user 是要评的材料：问题、工具查到的、
//! 回答；事件 RCA 是事件的事实与 RCA。材料按长度截断。模型给 1 到 5 分，换算成 0 到 1。

use serde_json::Value;

use super::ScoreTarget;
use crate::{
    agent::runs::TranscriptStep,
    shared::{Error, Result},
};

/// 评审 Prompt 的用途（与 baseline 里内置的 `evaluation.default` 一致）。
pub const EVALUATION_PURPOSE: &str = "evaluation";

/// 接在评审 Prompt 后面的输出格式。
pub const VERDICT_INSTRUCTION: &str = "Return only one JSON object without a Markdown fence: \
     {\"score\": <an integer from 1 (unusable) to 5 (excellent)>, \"reason\": \"<at most two \
     sentences on the main strength or flaw>\"}.";

const QUESTION_CHARS: usize = 4_000;
const ANSWER_CHARS: usize = 8_000;
const NOTE_CHARS: usize = 2_000;
/// 一个工具结果最多交给评审这么多字符，全部工具结果合起来最多这么多。
const RESULT_CHARS: usize = 1_500;
const FINDINGS_CHARS: usize = 12_000;
const ARGUMENT_CHARS: usize = 400;
const REASON_CHARS: usize = 500;

/// 要评的材料。
#[derive(Debug, Clone, Copy)]
pub struct Material<'a> {
    pub kind: ScoreTarget,
    /// 对话回答是问题；事件 RCA 是事件的事实。
    pub question: &'a str,
    /// 回答过程中的工具调用与结果。
    pub findings: &'a [TranscriptStep],
    pub answer: &'a str,
    /// 审核人对原回答的备注：回归集里的对象才有。
    pub note: Option<&'a str>,
}

/// 交给评审的 user 消息。
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

/// 工具调用一行一个，结果截断后缩进写在下面。晋级检查重新回答时也这样交给模型。
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

/// 写理由用的语言的名称，填进评审 Prompt 的 `{{output_language}}`。
pub fn output_language(language: &str) -> &'static str {
    if language.trim().to_ascii_lowercase().starts_with("zh") {
        "Simplified Chinese"
    } else {
        "English"
    }
}

/// 评审给的分。
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// 0 到 1。
    pub score: f64,
    /// 模型给的 1 到 5 分。
    pub points: u8,
    pub reason: String,
}

/// 读评审的回答：一个 JSON 对象，可以带围栏或前后的文字。
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

/// 最多 `chars` 个字符，截断处说明一下。
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
    use crate::agent::runs::TranscriptCall;

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
