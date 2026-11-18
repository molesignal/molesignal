// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The answer format is a JSON object the model returns as requested: conclusion, evidence,
//! possible causes, unconfirmed details, next steps, related links, and confidence.
//!
//! The conversation page displays answers in this format. The backend reads it here: notifications
//! and run records use only the conclusion, incident RCA uses Markdown, and scoring checks the
//! format and evidence. Answers outside this format (plain text or query-only output) are used as
//! is.

use serde_json::{Map, Value};

/// Fields that identify this format: at least one must be present.
const KEYS: [&str; 7] = [
    "summary",
    "anomaly_points",
    "evidence",
    "likely_causes",
    "limitations",
    "suggested_next_steps",
    "related_links",
];

/// A piece of evidence or a related link in the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cited {
    pub label: String,
    /// An in-product page address that starts with `/`.
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

    /// Model-provided confidence: a level word or a number from 0 to 1 (possibly a percentage).
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

/// The parsed answer.
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

/// Section headings and the confidence line for Markdown formatting, in the reader's language.
#[derive(Debug, Clone, Default)]
pub struct Headings {
    pub likely_causes: String,
    pub evidence: String,
    pub next_steps: String,
    pub limitations: String,
    pub related_links: String,
    /// A complete line for each of the three levels, such as `Confidence: High`.
    pub confidence_high: String,
    pub confidence_medium: String,
    pub confidence_low: String,
}

impl StructuredAnswer {
    /// Parses an answer; returns `None` if it does not match this format.
    /// An outer ```json fence is allowed.
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

    /// Formats as Markdown: the conclusion comes first, and each nonempty section has a heading.
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

/// The answer's conclusion: `summary` for this format, otherwise the original text trimmed.
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
                // Whitespace or parentheses in the address break the link, so output only text.
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
