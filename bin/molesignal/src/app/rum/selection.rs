// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What the explorer is looking at: a kind of row, a range of time, filters and words. The list,
//! the histogram and the field values of the explorer all select by exactly this, so the bars
//! count the rows the list pages through.

use super::reader::Span;
use crate::{
    domain::rum::event::RumStream,
    infra::rum::query::{
        Condition, Filter, asks_about_traffic, conditions, people_only, text_columns,
        text_conditions,
    },
    shared::{Error, Result, time::TimestampMicros},
};

/// Without a range, the last day.
const DEFAULT_WINDOW_MICROS: i64 = 24 * 60 * 60 * 1_000_000;

#[derive(Debug, Clone)]
pub struct Selection {
    pub stream: RumStream,
    pub span: Span,
    pub filters: Vec<Filter>,
    pub free_text: Vec<String>,
    /// Also the sessions no person ran, whatever the filters say: what the kinds of session are is
    /// asked of every kind.
    pub every_kind: bool,
}

impl Selection {
    pub fn new(
        kind: Option<&str>,
        from: Option<i64>,
        to: Option<i64>,
        filters: Vec<Filter>,
        free_text: Vec<String>,
    ) -> Result<Self> {
        let stream = stream_of(kind.unwrap_or_default())?;

        let to = to.unwrap_or_else(|| TimestampMicros::now().0);
        let from = from.unwrap_or_else(|| to.saturating_sub(DEFAULT_WINDOW_MICROS));

        if to <= from {
            return Err(Error::invalid("range end must be greater than start"));
        }

        Ok(Self {
            stream,
            span: Span { from, to },
            filters,
            free_text,
            every_kind: false,
        })
    }

    /// The conditions rows have to meet, with the words each in one of the columns text is looked
    /// for in. A filter or a word that cannot be asked of the stream fails the request. A selection
    /// is of the sessions people ran, unless its filters ask about what ran them.
    pub fn conditions(&self) -> Result<Vec<Condition>> {
        let mut found = conditions(self.stream, &self.filters)?;

        found.extend(text_conditions(text_columns(self.stream), &self.free_text)?);

        if !self.every_kind && !asks_about_traffic(&self.filters) {
            found.push(people_only());
        }

        Ok(found)
    }
}

/// The stream a page names by its collection (`views`, `long_tasks`).
pub fn stream_of(collection: &str) -> Result<RumStream> {
    RumStream::from_collection(collection.trim()).ok_or_else(|| {
        let known = RumStream::ALL.map(RumStream::collection).join(", ");

        Error::invalid(format!("type must be one of {known}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(field: &str, value: &str) -> Filter {
        Filter {
            field: field.into(),
            op: "=".into(),
            value: value.into(),
            quoted: false,
        }
    }

    #[test]
    fn a_collection_names_its_stream() {
        assert_eq!(stream_of("views").unwrap(), RumStream::Views);
        assert_eq!(stream_of(" long_tasks ").unwrap(), RumStream::LongTasks);

        let error = stream_of("view").unwrap_err().to_string();

        assert!(error.contains("sessions, views, actions"), "{error}");
        assert!(stream_of("").is_err());
    }

    #[test]
    fn without_a_range_the_last_day_is_selected() {
        let selection = Selection::new(Some("errors"), None, None, Vec::new(), Vec::new()).unwrap();

        assert_eq!(
            selection.span.to - selection.span.from,
            DEFAULT_WINDOW_MICROS
        );
    }

    #[test]
    fn the_range_must_run_forward() {
        assert!(
            Selection::new(Some("errors"), Some(10), Some(10), Vec::new(), Vec::new()).is_err()
        );
        assert!(Selection::new(Some("errors"), Some(10), Some(5), Vec::new(), Vec::new()).is_err());
        assert!(Selection::new(Some("errors"), Some(5), Some(10), Vec::new(), Vec::new()).is_ok());
    }

    #[test]
    fn a_selection_asks_for_a_session_no_person_ran_by_saying_so() {
        for (name, value) in [
            ("session.type", "synthetics"),
            ("synthetics.result_id", "task-1"),
            ("ci_test.test_execution_id", "run-1"),
        ] {
            let selection = Selection::new(
                Some("sessions"),
                Some(0),
                Some(10),
                vec![filter(name, value)],
                Vec::new(),
            )
            .unwrap();

            assert_eq!(selection.conditions().unwrap().len(), 1, "{name}");
        }
    }

    #[test]
    fn filters_and_words_become_conditions_of_the_stream() {
        let selection = Selection::new(
            Some("errors"),
            Some(0),
            Some(10),
            vec![filter("env", "prod")],
            vec!["timeout".into()],
        )
        .unwrap();

        let found = selection.conditions().unwrap();

        assert_eq!(found.len(), 3);
        assert_eq!(found[0].sql, "\"env\" = 'prod'");
        assert!(found[1].sql.contains("\"error.message\""));
        assert_eq!(found[2].sql, people_only().sql);

        let unknown = Selection::new(
            Some("errors"),
            Some(0),
            Some(10),
            vec![filter("view.url_path_nope", "x")],
            Vec::new(),
        )
        .unwrap();

        assert!(unknown.conditions().is_err());
    }
}
