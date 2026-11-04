// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a model is told about a session, and what is made of its answer.
//!
//! A session is told as lines, oldest first, each with its time from the start of the session: the
//! pages, what was done on them, what went wrong. The text has been through what the application
//! scrubs and the reader may see, like anything else read from the streams. A long session is told
//! by what matters most: the pages and what went wrong always, the rest as far as there is room.

use serde::Deserialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::{
    domain::rum::RumSummaryChapter,
    shared::{Error, Result},
};

/// The most lines a model is told of one session.
const MAX_LINES: usize = 400;
/// The longest line, in characters.
const MAX_LINE_CHARS: usize = 240;
/// The most chapters kept of an answer.
const MAX_CHAPTERS: usize = 8;
const MAX_TITLE_CHARS: usize = 80;
const MAX_CHAPTER_CHARS: usize = 400;
const MAX_SUMMARY_CHARS: usize = 2_000;

const NANOS_PER_MILLI: f64 = 1_000_000.0;
/// A request that took longer than this is worth telling.
const SLOW_REQUEST_NANOS: f64 = 3_000.0 * NANOS_PER_MILLI;
/// A long task that took longer than this is worth telling.
const LONG_TASK_NANOS: f64 = 500.0 * NANOS_PER_MILLI;

/// One thing that happened, as a line of what the model is told.
struct Line {
    at_micros: i64,
    text: String,
    /// The pages and what went wrong are told whatever the length of the session.
    notable: bool,
}

fn text<'a>(item: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    item.get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

fn number(item: &Map<String, Value>, name: &str) -> Option<f64> {
    item.get(name).and_then(Value::as_f64)
}

fn seconds(nanos: f64) -> String {
    format!("{:.1}s", nanos / NANOS_PER_MILLI / 1_000.0)
}

/// What an item of the timeline comes to as a line, if it is worth one.
fn line_of(item: &Map<String, Value>) -> Option<Line> {
    let at_micros = item.get("timestamp").and_then(Value::as_i64)?;

    let (text, notable) = match item.get("type").and_then(Value::as_str)? {
        "view" => {
            let page = text(item, "view.url_path").or_else(|| text(item, "view.name"))?;

            let loaded = number(item, "view.loading_time")
                .map(|nanos| format!(" (loaded in {})", seconds(nanos)))
                .unwrap_or_default();

            (format!("opened page {page}{loaded}"), true)
        }
        "action" => {
            let kind = text(item, "action.type").unwrap_or("custom");
            let name = text(item, "action.target.name")
                .or_else(|| text(item, "action.target.selector"))
                .unwrap_or("something");

            let signs = item
                .get("action.frustration.type")
                .and_then(|value| match value {
                    Value::Array(signs) => Some(
                        signs
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
                    Value::String(signs) => Some(signs.clone()),
                    _ => None,
                })
                .filter(|signs| !signs.is_empty() && signs != "[]");

            match signs {
                Some(signs) => (format!("{kind} on {name}; frustration: {signs}"), true),
                None => (format!("{kind} on {name}"), false),
            }
        }
        "error" => {
            let kind = text(item, "error.type").unwrap_or("Error");
            let message = text(item, "error.message").unwrap_or("");

            (format!("error {kind}: {message}"), true)
        }
        "resource" => {
            let status = number(item, "resource.status_code");
            let duration = number(item, "resource.duration").unwrap_or_default();
            let failed = status.is_some_and(|status| status == 0.0 || status >= 400.0);

            if !failed && duration < SLOW_REQUEST_NANOS {
                return None;
            }

            let method = text(item, "resource.method").unwrap_or("GET");
            let url = text(item, "resource.url_path_group")
                .or_else(|| text(item, "resource.url"))
                .unwrap_or("");

            let outcome = match status {
                Some(status) if failed => format!("failed with status {status}"),
                _ => format!("took {}", seconds(duration)),
            };

            (format!("request {method} {url} {outcome}"), failed)
        }
        "long_task" => {
            let duration = number(item, "long_task.duration")?;

            if duration < LONG_TASK_NANOS {
                return None;
            }

            (format!("the page froze for {}", seconds(duration)), false)
        }
        "vital" => {
            let name = text(item, "vital.name")?;
            let duration = number(item, "vital.duration").unwrap_or_default();

            (format!("{name} took {}", seconds(duration)), false)
        }
        "log" => {
            let level = text(item, "level").unwrap_or("info");

            if !matches!(level, "warn" | "error") {
                return None;
            }

            let message = text(item, "message")?;

            (format!("log {level}: {message}"), level == "error")
        }
        _ => return None,
    };

    Some(Line {
        at_micros,
        text: text.chars().take(MAX_LINE_CHARS).collect(),
        notable,
    })
}

/// What the model is told of a session that began at `start_micros`: the lines of `items`, oldest
/// first. `None` when there is nothing to tell.
pub fn input_of(start_micros: i64, items: &[Map<String, Value>]) -> Option<String> {
    let lines = items.iter().filter_map(line_of).collect::<Vec<_>>();

    if lines.is_empty() {
        return None;
    }

    let notable = lines.iter().filter(|line| line.notable).count();
    let room = MAX_LINES.saturating_sub(notable);

    let mut others = 0;

    let mut kept = lines
        .into_iter()
        .filter(|line| {
            if line.notable {
                return true;
            }

            others += 1;
            others <= room
        })
        .collect::<Vec<_>>();

    kept.truncate(MAX_LINES);
    kept.sort_by_key(|line| line.at_micros);

    Some(
        kept.iter()
            .map(|line| {
                let at = (line.at_micros - start_micros).max(0) as f64 / 1_000_000.0;

                format!("[{at:.0}s] {}", line.text)
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// The hash of what the model is told: the same words are not asked about twice.
pub fn digest_of(instructions: &str, input: &str) -> String {
    let mut hash = Sha256::new();

    hash.update(instructions.as_bytes());
    hash.update([0]);
    hash.update(input.as_bytes());

    hex::encode(&hash.finalize()[..16])
}

#[derive(Debug, Deserialize)]
struct Answer {
    summary: String,
    #[serde(default)]
    chapters: Vec<Chapter>,
}

#[derive(Debug, Deserialize)]
struct Chapter {
    at_seconds: f64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    summary: String,
}

/// What is made of an answer of the model. A model may put its JSON in a fence or say something
/// around it, so the object is looked for; a chapter outside the session, or without a title, is
/// left out, and chapters are kept in the order of the session.
pub fn read_answer(
    answer: &str,
    start_micros: i64,
    end_micros: i64,
) -> Result<(String, Vec<RumSummaryChapter>)> {
    let object = answer
        .find('{')
        .zip(answer.rfind('}'))
        .filter(|(open, close)| open < close)
        .map(|(open, close)| &answer[open..=close])
        .ok_or_else(|| Error::unavailable("the model did not answer with a summary"))?;

    let answer = serde_json::from_str::<Answer>(object)
        .map_err(|error| Error::unavailable(format!("the model's answer is not one: {error}")))?;

    let summary = answer.summary.trim();

    if summary.is_empty() {
        return Err(Error::unavailable("the model's summary is empty"));
    }

    let mut chapters = answer
        .chapters
        .into_iter()
        .filter(|chapter| chapter.at_seconds.is_finite() && chapter.at_seconds >= 0.0)
        .filter_map(|chapter| {
            let at_micros = start_micros + (chapter.at_seconds * 1_000_000.0) as i64;
            let title = chapter.title.trim();

            (!title.is_empty() && at_micros <= end_micros).then(|| RumSummaryChapter {
                at_time: at_micros,
                title: title.chars().take(MAX_TITLE_CHARS).collect(),
                summary: chapter
                    .summary
                    .trim()
                    .chars()
                    .take(MAX_CHAPTER_CHARS)
                    .collect(),
            })
        })
        .collect::<Vec<_>>();

    chapters.sort_by_key(|chapter| chapter.at_time);
    chapters.truncate(MAX_CHAPTERS);

    Ok((summary.chars().take(MAX_SUMMARY_CHARS).collect(), chapters))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn item(fields: Value) -> Map<String, Value> {
        fields.as_object().unwrap().clone()
    }

    #[test]
    fn a_session_is_told_as_lines_in_order() {
        let input = input_of(
            1_000_000,
            &[
                item(json!({
                    "type": "view",
                    "timestamp": 1_000_000,
                    "view.url_path": "/checkout",
                    "view.loading_time": 1_200_000_000_i64
                })),
                item(json!({
                    "type": "action",
                    "timestamp": 3_000_000,
                    "action.type": "click",
                    "action.target.name": "Pay now",
                    "action.frustration.type": ["rage_click"]
                })),
                item(json!({
                    "type": "error",
                    "timestamp": 4_000_000,
                    "error.type": "TypeError",
                    "error.message": "cart is null"
                })),
                item(json!({
                    "type": "resource",
                    "timestamp": 5_000_000,
                    "resource.method": "POST",
                    "resource.url_path_group": "/api/pay",
                    "resource.status_code": 500,
                    "resource.duration": 90_000_000
                })),
                item(json!({
                    "type": "log",
                    "timestamp": 6_000_000,
                    "level": "warn",
                    "message": "retrying"
                })),
            ],
        )
        .unwrap();

        assert_eq!(
            input,
            "[0s] opened page /checkout (loaded in 1.2s)\n\
             [2s] click on Pay now; frustration: rage_click\n\
             [3s] error TypeError: cart is null\n\
             [4s] request POST /api/pay failed with status 500\n\
             [5s] log warn: retrying"
        );
    }

    #[test]
    fn what_went_well_is_not_told() {
        let quiet = [
            item(json!({
                "type": "resource",
                "timestamp": 1,
                "resource.status_code": 200,
                "resource.duration": 90_000_000
            })),
            item(json!({"type": "log", "timestamp": 2, "level": "info", "message": "x"})),
            item(json!({"type": "long_task", "timestamp": 3, "long_task.duration": 60_000_000})),
        ];

        assert_eq!(input_of(0, &quiet), None);
    }

    #[test]
    fn a_long_session_keeps_what_went_wrong() {
        let mut items = (0..1_000)
            .map(|at| {
                item(json!({
                    "type": "action",
                    "timestamp": at,
                    "action.type": "click",
                    "action.target.name": "row"
                }))
            })
            .collect::<Vec<_>>();

        items.push(item(json!({
            "type": "error",
            "timestamp": 5_000,
            "error.message": "the last straw"
        })));

        let input = input_of(0, &items).unwrap();

        assert_eq!(input.lines().count(), MAX_LINES);
        assert!(input.contains("the last straw"));
    }

    #[test]
    fn an_answer_is_read_from_a_fence_and_its_chapters_are_put_in_order() {
        let answer = r#"Here it is:
```json
{"summary": " They paid. ", "chapters": [
  {"at_seconds": 30, "title": "Pay", "summary": "They paid."},
  {"at_seconds": 0, "title": "Browse", "summary": "They looked."},
  {"at_seconds": 9000, "title": "After the end"},
  {"at_seconds": -1, "title": "Before the start"},
  {"at_seconds": 40, "title": "  "}
]}
```"#;

        let (summary, chapters) = read_answer(answer, 1_000_000, 100_000_000).unwrap();

        assert_eq!(summary, "They paid.");
        assert_eq!(
            chapters
                .iter()
                .map(|chapter| (chapter.at_time, chapter.title.as_str()))
                .collect::<Vec<_>>(),
            [(1_000_000, "Browse"), (31_000_000, "Pay")]
        );
    }

    #[test]
    fn what_is_no_summary_is_refused() {
        assert!(read_answer("I cannot help with that.", 0, 10).is_err());
        assert!(read_answer(r#"{"chapters": []}"#, 0, 10).is_err());
        assert!(read_answer(r#"{"summary": "  "}"#, 0, 10).is_err());
    }

    #[test]
    fn the_same_words_have_the_same_digest() {
        assert_eq!(digest_of("a", "b"), digest_of("a", "b"));
        assert_ne!(digest_of("a", "b"), digest_of("ab", ""));
    }
}
