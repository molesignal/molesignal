// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The investigation guide of an alert rule: what to do first when one of its incidents is looked
//! into. The steps say what to check and in what order, the queries are run over the incident's
//! time range, and the dashboards are opened beside it. People read it on the incident; Mole Agent
//! reads it before it investigates.

use kernel::{Error, Result, ids::Id};
use serde::{Deserialize, Serialize};

use crate::domain::query::{QueryLanguage, StreamHint};

/// The longest the steps may be, in characters.
pub const MAX_GUIDE_STEPS_CHARS: usize = 8_000;

/// The most queries a guide holds.
pub const MAX_GUIDE_QUERIES: usize = 10;

/// The longest a query's name may be, in characters.
pub const MAX_GUIDE_QUERY_NAME_CHARS: usize = 120;

/// The longest a query's statement may be, in characters.
pub const MAX_GUIDE_STATEMENT_CHARS: usize = 4_000;

/// The most dashboards a guide links.
pub const MAX_GUIDE_DASHBOARDS: usize = 5;

/// The longest a stream name or a dashboard id may be, in characters.
const MAX_REFERENCE_CHARS: usize = 255;

/// What someone looking into one of the rule's incidents does first. Every part may be empty; a
/// rule saved before guides existed reads as an empty one.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleGuide {
    /// Markdown: what to check first and in what order.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub steps: String,
    /// Queries to run over the incident's time range, in the order they are listed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queries: Vec<GuideQuery>,
    /// Dashboards to open beside the incident.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dashboards: Vec<Id>,
}

/// One query of a guide.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideQuery {
    pub name: String,
    pub language: QueryLanguage,
    /// The stream a SQL query reads; a PromQL query may leave it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<StreamHint>,
    pub statement: String,
}

impl RuleGuide {
    /// Whether the guide says nothing at all.
    pub fn is_empty(&self) -> bool {
        self.steps.trim().is_empty() && self.queries.is_empty() && self.dashboards.is_empty()
    }

    /// Trims what was typed and checks the guide against its limits before it is saved: the
    /// steps, at most [`MAX_GUIDE_QUERIES`] named queries (a SQL one names its stream) and at
    /// most [`MAX_GUIDE_DASHBOARDS`] dashboards, each once.
    pub fn normalize(&mut self) -> Result<()> {
        self.steps = self.steps.trim().to_string();

        if self.steps.chars().count() > MAX_GUIDE_STEPS_CHARS {
            return Err(Error::invalid(format!(
                "guide.steps holds at most {MAX_GUIDE_STEPS_CHARS} characters"
            )));
        }

        if self.queries.len() > MAX_GUIDE_QUERIES {
            return Err(Error::invalid(format!(
                "guide.queries holds at most {MAX_GUIDE_QUERIES} queries"
            )));
        }

        for (index, query) in self.queries.iter_mut().enumerate() {
            query.normalize(index)?;
        }

        self.dashboards = normalize_dashboards(std::mem::take(&mut self.dashboards))?;
        Ok(())
    }
}

impl GuideQuery {
    fn normalize(&mut self, index: usize) -> Result<()> {
        self.name = self.name.trim().to_string();
        self.statement = self.statement.trim().to_string();

        let name_chars = self.name.chars().count();

        if name_chars == 0 || name_chars > MAX_GUIDE_QUERY_NAME_CHARS {
            return Err(Error::invalid(format!(
                "guide.queries[{index}].name must be 1 to {MAX_GUIDE_QUERY_NAME_CHARS} characters"
            )));
        }

        let statement_chars = self.statement.chars().count();

        if statement_chars == 0 || statement_chars > MAX_GUIDE_STATEMENT_CHARS {
            return Err(Error::invalid(format!(
                "guide.queries[{index}].statement must be 1 to {MAX_GUIDE_STATEMENT_CHARS} characters"
            )));
        }

        if let Some(stream) = &mut self.stream {
            stream.name = stream.name.trim().to_string();

            if stream.name.chars().count() > MAX_REFERENCE_CHARS {
                return Err(Error::invalid(format!(
                    "guide.queries[{index}].stream.name is longer than {MAX_REFERENCE_CHARS} characters"
                )));
            }
        }

        // An unnamed stream says nothing: a SQL query has to name one, a PromQL query goes without.
        if self
            .stream
            .as_ref()
            .is_some_and(|stream| stream.name.is_empty())
        {
            self.stream = None;
        }

        if self.language == QueryLanguage::Sql && self.stream.is_none() {
            return Err(Error::invalid(format!(
                "guide.queries[{index}] is SQL and has to name the stream it reads"
            )));
        }

        Ok(())
    }
}

/// The dashboards trimmed, without blanks and repeats, in the order given.
fn normalize_dashboards(dashboards: Vec<Id>) -> Result<Vec<Id>> {
    let mut kept: Vec<Id> = Vec::with_capacity(dashboards.len());

    for dashboard in dashboards {
        let id = dashboard.0.trim().to_string();

        if id.is_empty() || kept.iter().any(|known| known.0 == id) {
            continue;
        }

        if id.chars().count() > MAX_REFERENCE_CHARS {
            return Err(Error::invalid(format!(
                "guide.dashboards names an id longer than {MAX_REFERENCE_CHARS} characters"
            )));
        }

        kept.push(Id(id));
    }

    if kept.len() > MAX_GUIDE_DASHBOARDS {
        return Err(Error::invalid(format!(
            "guide.dashboards holds at most {MAX_GUIDE_DASHBOARDS} dashboards"
        )));
    }

    Ok(kept)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::stream::StreamType;

    fn sql(name: &str, stream: &str, statement: &str) -> GuideQuery {
        GuideQuery {
            name: name.into(),
            language: QueryLanguage::Sql,
            stream: Some(StreamHint {
                name: stream.into(),
                stream_type: StreamType::LOGS,
            }),
            statement: statement.into(),
        }
    }

    fn promql(name: &str, statement: &str) -> GuideQuery {
        GuideQuery {
            name: name.into(),
            language: QueryLanguage::Promql,
            stream: None,
            statement: statement.into(),
        }
    }

    #[test]
    fn a_rule_saved_before_guides_reads_as_an_empty_one() {
        let guide: RuleGuide = serde_json::from_value(json!({})).unwrap();

        assert!(guide.is_empty());
        assert_eq!(serde_json::to_value(&guide).unwrap(), json!({}));
    }

    #[test]
    fn a_guide_is_written_in_camel_case_and_read_back() {
        let guide = RuleGuide {
            steps: "1. Check the error rate by endpoint.".into(),
            queries: vec![sql("Errors", "app_logs", "SELECT * FROM app_logs")],
            dashboards: vec![Id("dash-1".into())],
        };

        let value = serde_json::to_value(&guide).unwrap();

        assert_eq!(value["queries"][0]["stream"]["streamType"], "logs");
        assert_eq!(value["queries"][0]["language"], "sql");
        assert_eq!(value["dashboards"], json!(["dash-1"]));

        let read: RuleGuide = serde_json::from_value(value).unwrap();
        assert_eq!(read.queries[0].name, "Errors");
        assert!(!read.is_empty());
    }

    #[test]
    fn normalizing_trims_what_was_typed() {
        let mut guide = RuleGuide {
            steps: "\n  Look at the gateway first.  \n".into(),
            queries: vec![sql("  Errors  ", " app_logs ", "  SELECT 1  ")],
            dashboards: vec![Id(" dash-1 ".into()), Id("dash-1".into()), Id("  ".into())],
        };

        guide.normalize().unwrap();

        assert_eq!(guide.steps, "Look at the gateway first.");
        assert_eq!(guide.queries[0].name, "Errors");
        assert_eq!(guide.queries[0].statement, "SELECT 1");
        assert_eq!(guide.queries[0].stream.as_ref().unwrap().name, "app_logs");
        assert_eq!(guide.dashboards, vec![Id("dash-1".into())]);
    }

    #[test]
    fn steps_have_a_limit() {
        let mut guide = RuleGuide {
            steps: "x".repeat(MAX_GUIDE_STEPS_CHARS + 1),
            ..RuleGuide::default()
        };

        assert!(guide.normalize().is_err());

        // Characters are counted, not bytes.
        let mut guide = RuleGuide {
            steps: "查".repeat(MAX_GUIDE_STEPS_CHARS),
            ..RuleGuide::default()
        };

        assert!(guide.normalize().is_ok());
    }

    #[test]
    fn queries_have_a_limit_names_and_statements() {
        let mut guide = RuleGuide {
            queries: (0..=MAX_GUIDE_QUERIES)
                .map(|index| promql(&format!("q{index}"), "up"))
                .collect(),
            ..RuleGuide::default()
        };

        assert!(guide.normalize().is_err());

        let mut unnamed = RuleGuide {
            queries: vec![promql("  ", "up")],
            ..RuleGuide::default()
        };

        let error = unnamed.normalize().unwrap_err().to_string();
        assert!(error.contains("guide.queries[0].name"), "{error}");

        let mut empty = RuleGuide {
            queries: vec![promql("Up", " ")],
            ..RuleGuide::default()
        };

        assert!(empty.normalize().is_err());

        let mut long = RuleGuide {
            queries: vec![promql("Up", &"x".repeat(MAX_GUIDE_STATEMENT_CHARS + 1))],
            ..RuleGuide::default()
        };

        assert!(long.normalize().is_err());
    }

    #[test]
    fn a_sql_query_names_its_stream_and_a_promql_one_need_not() {
        let mut sql_without_stream = RuleGuide {
            queries: vec![sql("Errors", "  ", "SELECT 1")],
            ..RuleGuide::default()
        };

        let error = sql_without_stream.normalize().unwrap_err().to_string();
        assert!(error.contains("stream"), "{error}");

        let mut promql_only = RuleGuide {
            queries: vec![promql("Rate", "sum(rate(http_requests_total[5m]))")],
            ..RuleGuide::default()
        };

        assert!(promql_only.normalize().is_ok());
    }

    #[test]
    fn dashboards_have_a_limit_after_repeats_are_dropped() {
        let mut repeated = RuleGuide {
            dashboards: (0..10).map(|_| Id("dash".into())).collect(),
            ..RuleGuide::default()
        };

        assert!(repeated.normalize().is_ok());
        assert_eq!(repeated.dashboards.len(), 1);

        let mut many = RuleGuide {
            dashboards: (0..=MAX_GUIDE_DASHBOARDS)
                .map(|index| Id(format!("dash-{index}")))
                .collect(),
            ..RuleGuide::default()
        };

        assert!(many.normalize().is_err());
    }
}
