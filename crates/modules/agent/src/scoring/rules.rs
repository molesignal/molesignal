// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Rule-based scoring assigns each dimension a score from 0 to 1 based on recorded facts without
//! calling a model; the overall score is the average of the available dimensions.
//!
//! Facts for each dimension ([`DimensionScore::detail`]):
//!
//! - `tool_success`: `{failed, total}`. Absent for answers without tool calls.
//! - `evidence`: `{cited, backed}`, the number of pieces of evidence cited in the answer and
//!   whether successful tool calls support it. Present only when the answer has the required shape
//!   and tools are called. For incident RCA, `{cited, given, unknown}`: the labels the summary
//!   cites, the evidence it was given, and the labels cited that it was not given; absent when it
//!   was given none and cites none.
//! - `query_validity`: `{invalid, total, firstInvalid}`, SQL and PromQL in tool calls. Absent when
//!   there are no queries.
//! - `answer_format`: `{structured}`. Absent for query-only turns and incident RCA.
//! - `completion`: `{status}`, the Run outcome; blocked automation runs are recorded as `blocked`.
//! - `feedback`: `{rating, reasons}`. Absent when no one provides feedback.

use serde_json::{Value, json};

use super::{DimensionScore, RuleDimension};
use crate::{
    answer::StructuredAnswer,
    feedback::{FeedbackRating, FeedbackReason},
    runs::{RunStatus, TranscriptStep},
};

/// The maximum number of characters recorded in the facts for the first unparseable query.
const QUERY_EXCERPT_CHARS: usize = 300;

/// Checks whether a query can be parsed: SQL uses the query-language parser, and PromQL uses the
/// PromQL parser, both supplied by the caller.
pub trait QuerySyntax: Send + Sync {
    fn sql_parses(&self, sql: &str) -> bool;
    fn promql_parses(&self, promql: &str) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryLanguage {
    Sql,
    PromQl,
}

/// A query included in a tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolQuery {
    pub language: QueryLanguage,
    pub text: String,
}

/// Facts about a chat answer.
#[derive(Debug, Clone, Copy)]
pub struct AnswerFacts<'a> {
    pub content: &'a str,
    pub steps: &'a [TranscriptStep],
    /// Requires the answer to have a JSON shape; not required for query-only turns.
    pub structured_expected: bool,
    pub status: RunStatus,
    /// Automation requires a human response, confirmation, or additional credentials along the way,
    /// but no one is available to handle it.
    pub blocked: bool,
    pub feedback: Option<(FeedbackRating, &'a [FeedbackReason])>,
}

/// Rule-based scoring for a chat answer.
pub fn score_answer(facts: &AnswerFacts<'_>, syntax: &dyn QuerySyntax) -> Vec<DimensionScore> {
    let calls = facts
        .steps
        .iter()
        .flat_map(|step| step.calls.iter())
        .collect::<Vec<_>>();
    let failed = calls.iter().filter(|call| call.is_error).count();
    let answer = StructuredAnswer::parse(facts.content);
    let mut dimensions = Vec::new();

    if !calls.is_empty() {
        dimensions.push(DimensionScore {
            dimension: RuleDimension::ToolSuccess,
            score: 1.0 - failed as f64 / calls.len() as f64,
            detail: json!({ "failed": failed, "total": calls.len() }),
        });
    }

    if facts.structured_expected
        && !calls.is_empty()
        && let Some(answer) = &answer
    {
        let cited = answer.evidence.len();
        let backed = failed < calls.len();

        let score = match (cited, backed) {
            (0, _) => 0.0,
            (_, false) => 0.5,
            (1, true) => 0.75,
            _ => 1.0,
        };

        dimensions.push(DimensionScore {
            dimension: RuleDimension::Evidence,
            score,
            detail: json!({ "cited": cited, "backed": backed }),
        });
    }

    let queries = queries(facts.steps);

    if !queries.is_empty() {
        let invalid = queries
            .iter()
            .filter(|query| match query.language {
                QueryLanguage::Sql => !syntax.sql_parses(&query.text),
                QueryLanguage::PromQl => !syntax.promql_parses(&query.text),
            })
            .collect::<Vec<_>>();

        dimensions.push(DimensionScore {
            dimension: RuleDimension::QueryValidity,
            score: 1.0 - invalid.len() as f64 / queries.len() as f64,
            detail: json!({
                "invalid": invalid.len(),
                "total": queries.len(),
                "firstInvalid": invalid
                    .first()
                    .map(|query| query.text.chars().take(QUERY_EXCERPT_CHARS).collect::<String>()),
            }),
        });
    }

    if facts.structured_expected {
        let structured = answer.is_some_and(|answer| !answer.summary.is_empty());

        dimensions.push(DimensionScore {
            dimension: RuleDimension::AnswerFormat,
            score: if structured { 1.0 } else { 0.0 },
            detail: json!({ "structured": structured }),
        });
    }

    dimensions.push(completion(facts.status, facts.blocked, facts.content));
    dimensions.extend(feedback(facts.feedback));
    dimensions
}

/// Rule-based scoring for an incident RCA: whether it is written and people's feedback.
pub fn score_rca(
    summary: &str,
    evidence: &[String],
    feedback_given: Option<(FeedbackRating, &[FeedbackReason])>,
) -> Vec<DimensionScore> {
    let status = if summary.trim().is_empty() {
        RunStatus::Failed
    } else {
        RunStatus::Succeeded
    };

    let mut dimensions = vec![completion(status, false, summary)];
    dimensions.extend(rca_citations(summary, evidence));
    dimensions.extend(feedback(feedback_given));
    dimensions
}

/// Whether an analysis cites the evidence it was given, and only that. Given evidence, citing a
/// label it was not given scores 0 and citing none 0.25. Given none, citing anything is made up
/// and scores 0; citing nothing is not judged.
fn rca_citations(summary: &str, evidence: &[String]) -> Option<DimensionScore> {
    let cited = cited_labels(summary);
    let unknown = cited
        .iter()
        .filter(|label| !evidence.contains(label))
        .count();

    if evidence.is_empty() && cited.is_empty() {
        return None;
    }

    let score = if unknown > 0 {
        0.0
    } else if cited.is_empty() {
        0.25
    } else {
        1.0
    };

    Some(DimensionScore {
        dimension: RuleDimension::Evidence,
        score,
        detail: json!({ "cited": cited.len(), "given": evidence.len(), "unknown": unknown }),
    })
}

/// The evidence labels a text cites, such as `[E2]`, each once in the order first cited.
pub fn cited_labels(text: &str) -> Vec<String> {
    let mut labels = Vec::new();

    for (index, _) in text.match_indices("[E") {
        let rest = &text[index + 2..];
        let digits = rest
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();

        if digits.is_empty() || !rest[digits.len()..].starts_with(']') {
            continue;
        }

        let label = format!("E{digits}");

        if !labels.contains(&label) {
            labels.push(label);
        }
    }

    labels
}

/// The average of the available dimensions.
pub fn mean(dimensions: &[DimensionScore]) -> f64 {
    if dimensions.is_empty() {
        return 0.0;
    }

    dimensions
        .iter()
        .map(|dimension| dimension.score)
        .sum::<f64>()
        / dimensions.len() as f64
}

/// Queries included in tool calls: `sql`, `promql` parameters, or paired `language` and
/// `statement`; for tools called through `tools_call`, inspect the forwarded arguments.
pub fn queries(steps: &[TranscriptStep]) -> Vec<ToolQuery> {
    let mut found = Vec::new();

    for call in steps.iter().flat_map(|step| step.calls.iter()) {
        collect(&call.name, &call.arguments, &mut found);
    }

    found
}

fn collect(name: &str, arguments: &Value, found: &mut Vec<ToolQuery>) {
    if name == "tools_call" {
        if let (Some(inner), Some(arguments)) = (
            arguments.get("name").and_then(Value::as_str),
            arguments.get("arguments"),
        ) {
            collect(inner, arguments, found);
        }

        return;
    }

    let text = |key: &str| {
        arguments
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    };

    if let Some(sql) = text("sql") {
        found.push(ToolQuery {
            language: QueryLanguage::Sql,
            text: sql,
        });
    }

    if let Some(promql) = text("promql") {
        found.push(ToolQuery {
            language: QueryLanguage::PromQl,
            text: promql,
        });
    }

    let language = match arguments.get("language").and_then(Value::as_str) {
        Some("sql") => Some(QueryLanguage::Sql),
        Some("promql") => Some(QueryLanguage::PromQl),
        _ => None,
    };

    if let (Some(language), Some(statement)) = (language, text("statement")) {
        found.push(ToolQuery {
            language,
            text: statement,
        });
    }
}

fn completion(status: RunStatus, blocked: bool, content: &str) -> DimensionScore {
    let (score, label) = match status {
        RunStatus::Succeeded if content.trim().is_empty() => (0.0, "empty"),
        RunStatus::Succeeded if blocked => (0.5, "blocked"),
        RunStatus::Succeeded => (1.0, status.as_str()),
        other => (0.0, other.as_str()),
    };

    DimensionScore {
        dimension: RuleDimension::Completion,
        score,
        detail: json!({ "status": label }),
    }
}

fn feedback(given: Option<(FeedbackRating, &[FeedbackReason])>) -> Option<DimensionScore> {
    let (rating, reasons) = given?;

    Some(DimensionScore {
        dimension: RuleDimension::Feedback,
        score: match rating {
            FeedbackRating::Up => 1.0,
            FeedbackRating::Down => 0.0,
        },
        detail: json!({
            "rating": rating.as_str(),
            "reasons": reasons.iter().map(|reason| reason.as_str()).collect::<Vec<_>>(),
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runs::TranscriptCall;

    struct Syntax;

    impl QuerySyntax for Syntax {
        fn sql_parses(&self, sql: &str) -> bool {
            sql.to_ascii_uppercase().starts_with("SELECT")
        }

        fn promql_parses(&self, promql: &str) -> bool {
            !promql.contains("label_values")
        }
    }

    fn call(name: &str, arguments: Value, is_error: bool) -> TranscriptCall {
        TranscriptCall {
            id: format!("c-{name}"),
            name: name.into(),
            arguments,
            result: Some("{}".into()),
            truncated: false,
            is_error,
        }
    }

    fn facts<'a>(content: &'a str, steps: &'a [TranscriptStep]) -> AnswerFacts<'a> {
        AnswerFacts {
            content,
            steps,
            structured_expected: true,
            status: RunStatus::Succeeded,
            blocked: false,
            feedback: None,
        }
    }

    fn dimension(dimensions: &[DimensionScore], wanted: RuleDimension) -> Option<f64> {
        dimensions
            .iter()
            .find(|dimension| dimension.dimension == wanted)
            .map(|dimension| dimension.score)
    }

    #[test]
    fn a_grounded_answer_scores_well_on_every_dimension() {
        let steps = vec![TranscriptStep {
            text: String::new(),
            calls: vec![
                call(
                    "query_logs",
                    json!({ "sql": "SELECT count(*) FROM nginx" }),
                    false,
                ),
                call(
                    "query_metrics",
                    json!({ "promql": "rate(http_requests_total[5m])" }),
                    false,
                ),
            ],
        }];
        let content = r#"{"summary":"5xx rose after the deploy.","evidence":[{"label":"5xx at 12%"},{"label":"p99 doubled"}]}"#;
        let scored = score_answer(&facts(content, &steps), &Syntax);

        assert_eq!(scored.len(), 5);
        assert_eq!(mean(&scored), 1.0);
        assert_eq!(dimension(&scored, RuleDimension::QueryValidity), Some(1.0));
    }

    #[test]
    fn failing_tools_bad_queries_and_plain_text_lower_the_score() {
        let steps = vec![TranscriptStep {
            text: "Looking".into(),
            calls: vec![
                call(
                    "tools_call",
                    json!({ "name": "query_metrics", "arguments": { "promql": "label_values(up, job)" } }),
                    true,
                ),
                call("query_logs", json!({ "sql": "SELECT 1" }), false),
            ],
        }];
        let scored = score_answer(&facts("The service looks fine.", &steps), &Syntax);

        assert_eq!(dimension(&scored, RuleDimension::ToolSuccess), Some(0.5));
        assert_eq!(dimension(&scored, RuleDimension::QueryValidity), Some(0.5));
        assert_eq!(dimension(&scored, RuleDimension::AnswerFormat), Some(0.0));
        assert_eq!(dimension(&scored, RuleDimension::Evidence), None);

        let validity = scored
            .iter()
            .find(|dimension| dimension.dimension == RuleDimension::QueryValidity)
            .unwrap();
        assert_eq!(validity.detail["firstInvalid"], "label_values(up, job)");
    }

    #[test]
    fn evidence_without_successful_tools_is_half_trusted() {
        let steps = vec![TranscriptStep {
            text: String::new(),
            calls: vec![call("query_logs", json!({ "sql": "SELECT 1" }), true)],
        }];
        let content = r#"{"summary":"It was the cache.","evidence":[{"label":"cache misses"}]}"#;
        let scored = score_answer(&facts(content, &steps), &Syntax);

        assert_eq!(dimension(&scored, RuleDimension::Evidence), Some(0.5));
    }

    #[test]
    fn a_conversational_answer_is_scored_on_format_completion_and_feedback() {
        let reasons = [FeedbackReason::OffTopic];
        let answer = AnswerFacts {
            feedback: Some((FeedbackRating::Down, &reasons)),
            structured_expected: false,
            ..facts("SELECT count(*) FROM nginx", &[])
        };
        let scored = score_answer(&answer, &Syntax);

        assert_eq!(
            scored
                .iter()
                .map(|dimension| dimension.dimension)
                .collect::<Vec<_>>(),
            vec![RuleDimension::Completion, RuleDimension::Feedback]
        );
        assert_eq!(mean(&scored), 0.5);
        assert_eq!(scored[1].detail["reasons"], json!(["off_topic"]));
    }

    #[test]
    fn blocked_failed_and_empty_runs_count_as_unfinished() {
        let blocked = AnswerFacts {
            blocked: true,
            ..facts(r#"{"summary":"Done."}"#, &[])
        };
        assert_eq!(
            dimension(&score_answer(&blocked, &Syntax), RuleDimension::Completion),
            Some(0.5)
        );

        let failed = AnswerFacts {
            status: RunStatus::Failed,
            ..facts("partial", &[])
        };
        assert_eq!(
            dimension(&score_answer(&failed, &Syntax), RuleDimension::Completion),
            Some(0.0)
        );

        assert_eq!(mean(&score_rca("", &[], None)), 0.0);
        assert_eq!(
            mean(&score_rca(
                "The pool ran out.",
                &[],
                Some((FeedbackRating::Up, &[]))
            )),
            1.0
        );
    }

    #[test]
    fn an_rca_given_evidence_cites_it_and_nothing_else() {
        let given = ["E1".to_owned(), "E2".to_owned()];
        let citing =
            |summary: &str| dimension(&score_rca(summary, &given, None), RuleDimension::Evidence);

        assert_eq!(
            citing("The pool ran out [E2], after the deploy [E1]."),
            Some(1.0)
        );
        assert_eq!(citing("The pool ran out."), Some(0.25));
        assert_eq!(citing("The pool ran out [E7]."), Some(0.0));
    }

    #[test]
    fn an_rca_given_no_evidence_makes_none_up() {
        assert_eq!(
            dimension(
                &score_rca("The cache failed [E1].", &[], None),
                RuleDimension::Evidence
            ),
            Some(0.0)
        );
        assert_eq!(
            dimension(
                &score_rca("The evidence is not enough to tell.", &[], None),
                RuleDimension::Evidence
            ),
            None
        );
    }

    #[test]
    fn labels_are_read_once_each_and_only_whole() {
        assert_eq!(
            cited_labels("[E2] then [E10], [E2] again, [E] and [Ex1] [E3"),
            vec!["E2", "E10"]
        );
    }

    #[test]
    fn queries_come_from_sql_promql_and_language_statement_pairs() {
        let steps = vec![TranscriptStep {
            text: String::new(),
            calls: vec![
                call(
                    "submit_search_job",
                    json!({ "language": "promql", "statement": "up" }),
                    false,
                ),
                call("list_streams", json!({}), false),
            ],
        }];

        assert_eq!(
            queries(&steps),
            vec![ToolQuery {
                language: QueryLanguage::PromQl,
                text: "up".into(),
            }]
        );
    }
}
