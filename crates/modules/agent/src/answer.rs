// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 回答的形状：模型按要求返回的一个 JSON 对象——结论、证据、可能的原因、没能确认的、下一步、
//! 相关链接与置信度。
//!
//! 对话页按这个形状显示回答。后端在这里读它：通知与运行记录只用结论，写成事件的 RCA 时排成
//! Markdown，打分时看格式与证据。不是这个形状的回答（纯文本、只生成查询）原样使用。

use serde_json::{Map, Value};

/// 认得出是这个形状的字段：至少有一个。
const KEYS: [&str; 7] = [
    "summary",
    "anomaly_points",
    "evidence",
    "likely_causes",
    "limitations",
    "suggested_next_steps",
    "related_links",
];

/// 回答里的一条证据或一个相关链接。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cited {
    pub label: String,
    /// 产品内的页面地址，以 `/` 开头。
    pub route: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    High,
    Medium,
    Low,
}

impl Confidence {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }

    /// 模型写的置信度：等级的词，或 0 到 1（也可能是百分数）的数。
    fn read(value: &Value) -> Option<Self> {
        if let Some(word) = value.as_str() {
            return match word.trim().to_ascii_lowercase().as_str() {
                "high" => Some(Self::High),
                "medium" => Some(Self::Medium),
                "low" => Some(Self::Low),
                _ => None,
            };
        }

        let number = value.as_f64()?;
        let ratio = if number > 1.0 { number / 100.0 } else { number };

        Some(if ratio >= 0.75 {
            Self::High
        } else if ratio >= 0.4 {
            Self::Medium
        } else {
            Self::Low
        })
    }
}

/// 读出来的回答。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StructuredAnswer {
    pub summary: String,
    pub evidence: Vec<Cited>,
    pub likely_causes: Vec<String>,
    pub limitations: Vec<String>,
    pub next_steps: Vec<String>,
    pub related_links: Vec<Cited>,
    pub confidence: Option<Confidence>,
}

/// 排成 Markdown 时各部分的标题与置信度那一行，用读者的语言。
#[derive(Debug, Clone, Default)]
pub struct Headings {
    pub likely_causes: String,
    pub evidence: String,
    pub next_steps: String,
    pub limitations: String,
    pub related_links: String,
    /// 三个等级各自的一整行，例如「置信度：高」。
    pub confidence_high: String,
    pub confidence_medium: String,
    pub confidence_low: String,
}

impl StructuredAnswer {
    /// 读一条回答；不是这个形状时为 `None`。外面可以套一层 ```json 围栏。
    pub fn parse(content: &str) -> Option<Self> {
        let mut candidates = Vec::new();

        if let Some(fenced) = fenced(content) {
            candidates.push(fenced);
        }

        candidates.push(content);

        candidates.into_iter().find_map(|candidate| {
            let trimmed = candidate.trim();

            if !trimmed.starts_with('{') {
                return None;
            }

            let object = serde_json::from_str::<Value>(trimmed).ok()?;
            let object = object.as_object()?;

            KEYS.iter()
                .any(|key| object.contains_key(*key))
                .then(|| Self::from_object(object))
        })
    }

    fn from_object(object: &Map<String, Value>) -> Self {
        Self {
            summary: text(object.get("summary")),
            evidence: cited(object.get("evidence")),
            likely_causes: texts(object.get("likely_causes")),
            limitations: texts(object.get("limitations")),
            next_steps: texts(object.get("suggested_next_steps")),
            related_links: cited(object.get("related_links")),
            confidence: object.get("confidence").and_then(Confidence::read),
        }
    }

    /// 排成 Markdown：结论在前，各部分有内容时各带一个标题。
    pub fn to_markdown(&self, headings: &Headings) -> String {
        let mut parts = Vec::new();

        if !self.summary.is_empty() {
            parts.push(self.summary.clone());
        }

        let sections = [
            (&headings.likely_causes, bullets(&self.likely_causes)),
            (&headings.evidence, links(&self.evidence)),
            (&headings.next_steps, numbered(&self.next_steps)),
            (&headings.limitations, bullets(&self.limitations)),
            (&headings.related_links, links(&self.related_links)),
        ];

        for (heading, body) in sections {
            if !body.is_empty() {
                parts.push(format!("### {heading}\n\n{body}"));
            }
        }

        if let Some(confidence) = self.confidence {
            parts.push(
                match confidence {
                    Confidence::High => &headings.confidence_high,
                    Confidence::Medium => &headings.confidence_medium,
                    Confidence::Low => &headings.confidence_low,
                }
                .clone(),
            );
        }

        parts.join("\n\n")
    }
}

/// 回答的结论：是这个形状时取 `summary`，否则是去掉首尾空白的原文。
pub fn conclusion(content: &str) -> String {
    match StructuredAnswer::parse(content) {
        Some(answer) if !answer.summary.is_empty() => answer.summary,
        _ => content.trim().to_string(),
    }
}

fn fenced(content: &str) -> Option<&str> {
    let open = content.find("```")?;
    let after = &content[open + 3..];
    let body_start = after.find('\n')? + 1;
    let body = &after[body_start..];
    let close = body.find("```")?;

    Some(&body[..close])
}

fn text(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string()
}

fn texts(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn cited(value: Option<&Value>) -> Vec<Cited> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_object)
                .filter_map(|item| {
                    let label = text(item.get("label"));

                    if label.is_empty() {
                        return None;
                    }

                    let route = item
                        .get("route")
                        .or_else(|| item.get("href"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|route| route.starts_with('/'))
                        .map(str::to_string);

                    Some(Cited { label, route })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn bullets(items: &[String]) -> String {
    items
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn numbered(items: &[String]) -> String {
    items
        .iter()
        .enumerate()
        .map(|(index, item)| format!("{}. {item}", index + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

fn links(items: &[Cited]) -> String {
    items
        .iter()
        .map(|item| {
            let label = item.label.replace('[', "\\[").replace(']', "\\]");

            match item.route.as_deref() {
                // 地址里有空白或括号时链接会断开，只写文字。
                Some(route) if !route.contains([' ', '(', ')']) => {
                    format!("- [{label}]({route})")
                }
                _ => format!("- {label}"),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headings() -> Headings {
        Headings {
            likely_causes: "Likely causes".into(),
            evidence: "Evidence".into(),
            next_steps: "Next steps".into(),
            limitations: "Not confirmed".into(),
            related_links: "Related".into(),
            confidence_high: "Confidence: high".into(),
            confidence_medium: "Confidence: medium".into(),
            confidence_low: "Confidence: low".into(),
        }
    }

    #[test]
    fn reads_a_structured_answer_in_or_out_of_a_fence() {
        let plain = r#"{"summary":" Checkout is failing. ","evidence":[{"label":"5xx rose to 12%","route":"/logs?stream=nginx"},{"label":""}],"likely_causes":["A bad deploy"],"confidence":"high"}"#;
        let answer = StructuredAnswer::parse(plain).unwrap();

        assert_eq!(answer.summary, "Checkout is failing.");
        assert_eq!(
            answer.evidence,
            vec![Cited {
                label: "5xx rose to 12%".into(),
                route: Some("/logs?stream=nginx".into()),
            }]
        );
        assert_eq!(answer.confidence, Some(Confidence::High));

        let fenced = format!("Here it is:\n```json\n{plain}\n```");
        assert_eq!(StructuredAnswer::parse(&fenced), Some(answer));
    }

    #[test]
    fn plain_text_and_other_json_are_not_answers() {
        assert_eq!(StructuredAnswer::parse("Everything is fine."), None);
        assert_eq!(StructuredAnswer::parse(r#"{"query":"up"}"#), None);
        assert_eq!(conclusion("  Everything is fine.  "), "Everything is fine.");
        assert_eq!(
            conclusion(r#"{"summary":"Short.","evidence":[]}"#),
            "Short."
        );
    }

    #[test]
    fn numeric_confidence_is_read_as_a_level() {
        let read = |value: Value| Confidence::read(&value);

        assert_eq!(read(serde_json::json!(0.9)), Some(Confidence::High));
        assert_eq!(read(serde_json::json!(55)), Some(Confidence::Medium));
        assert_eq!(read(serde_json::json!(0.1)), Some(Confidence::Low));
        assert_eq!(read(serde_json::json!("unsure")), None);
    }

    #[test]
    fn writes_markdown_with_a_heading_per_filled_section() {
        let answer = StructuredAnswer {
            summary: "The database ran out of connections.".into(),
            evidence: vec![
                Cited {
                    label: "Pool [max] reached".into(),
                    route: Some("/metrics?q=pool".into()),
                },
                Cited {
                    label: "Errors in api".into(),
                    route: Some("/logs?q=a b".into()),
                },
            ],
            next_steps: vec!["Raise the pool size".into(), "Watch the latency".into()],
            confidence: Some(Confidence::Medium),
            ..StructuredAnswer::default()
        };

        assert_eq!(
            answer.to_markdown(&headings()),
            "The database ran out of connections.\n\n\
             ### Evidence\n\n- [Pool \\[max\\] reached](/metrics?q=pool)\n- Errors in api\n\n\
             ### Next steps\n\n1. Raise the pool size\n2. Watch the latency\n\n\
             Confidence: medium"
        );
    }
}
