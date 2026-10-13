// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 从模型写的报告里读出结论等级和一句话结论。
//!
//! 报告按约定是一个 JSON 对象，`level` 是 `normal` / `attention` / `abnormal` 之一。模型没有
//! 照做（写成了散文、漏了字段、写了别的词）时不去猜：等级记为未判定，由通知规则按异常对待。

use serde_json::Value;

use crate::domain::log_inspection::InspectionLevel;

/// 列表里显示的结论，最多这么多字符。
const SUMMARY_CHARS: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub level: InspectionLevel,
    /// 报告里的一句话结论；报告不是约定的形状时没有。
    pub summary: Option<String>,
}

pub fn read_verdict(report: &str) -> Verdict {
    let Some(answer) = report_object(report) else {
        return Verdict {
            level: InspectionLevel::Unknown,
            summary: None,
        };
    };

    let level = match answer.get("level").and_then(Value::as_str).map(str::trim) {
        Some(level) if level.eq_ignore_ascii_case("normal") => InspectionLevel::Normal,
        Some(level) if level.eq_ignore_ascii_case("attention") => InspectionLevel::Attention,
        Some(level) if level.eq_ignore_ascii_case("abnormal") => InspectionLevel::Abnormal,
        _ => InspectionLevel::Unknown,
    };

    let summary = answer
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
        .map(|summary| summary.chars().take(SUMMARY_CHARS).collect());

    Verdict { level, summary }
}

/// 报告里的那个 JSON 对象：整段就是、包在 Markdown 代码块里，或前后带了几句话。
fn report_object(report: &str) -> Option<Value> {
    let text = report.trim();

    let fenced = text
        .split_once("```")
        .and_then(|(_, rest)| rest.split_once("```"))
        .map(|(block, _)| block.trim_start_matches("json").trim());

    let braced = match (text.find('{'), text.rfind('}')) {
        (Some(open), Some(close)) if open < close => Some(&text[open..=close]),
        _ => None,
    };

    [Some(text), fenced, braced]
        .into_iter()
        .flatten()
        .filter_map(|candidate| serde_json::from_str::<Value>(candidate).ok())
        .find(Value::is_object)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_level_and_summary_are_read_from_the_report() {
        let verdict = read_verdict(
            r#"{"level":"abnormal","summary":" payment-service errors tripled after 09:10. ","evidence":[]}"#,
        );

        assert_eq!(verdict.level, InspectionLevel::Abnormal);
        assert_eq!(
            verdict.summary.as_deref(),
            Some("payment-service errors tripled after 09:10.")
        );
    }

    #[test]
    fn a_report_in_a_code_block_or_with_words_around_it_is_still_read() {
        let fenced = "```json\n{\"level\": \"Attention\", \"summary\": \"slow queries\"}\n```";
        assert_eq!(read_verdict(fenced).level, InspectionLevel::Attention);

        let wrapped = "Here is the report:\n{\"level\":\"normal\",\"summary\":\"quiet\"}\nDone.";
        let verdict = read_verdict(wrapped);
        assert_eq!(verdict.level, InspectionLevel::Normal);
        assert_eq!(verdict.summary.as_deref(), Some("quiet"));
    }

    #[test]
    fn a_report_without_a_readable_level_has_no_verdict() {
        for report in [
            "The logs look fine to me.",
            r#"{"summary":"no level given"}"#,
            r#"{"level":"critical","summary":"a word that is not a level"}"#,
            r#"{"level":2,"summary":"a number"}"#,
            r#"["level","abnormal"]"#,
            "",
        ] {
            assert_eq!(
                read_verdict(report).level,
                InspectionLevel::Unknown,
                "{report}"
            );
        }

        // 等级读不出来，结论照样留下。
        assert_eq!(
            read_verdict(r#"{"summary":"no level given"}"#)
                .summary
                .as_deref(),
            Some("no level given")
        );
    }

    #[test]
    fn a_long_summary_is_cut_for_the_list() {
        let report = format!(r#"{{"level":"normal","summary":"{}"}}"#, "长".repeat(900));

        assert_eq!(
            read_verdict(&report).summary.unwrap().chars().count(),
            SUMMARY_CHARS
        );
    }
}
