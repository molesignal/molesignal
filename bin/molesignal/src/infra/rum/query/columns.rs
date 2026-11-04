// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What the explorer reads of each stream when it is not told otherwise.

use crate::{
    domain::{
        rum::event::{RumStream, field, fields},
        stream::FieldType,
    },
    shared::{Error, Result},
};

const MAX_COLUMNS: usize = 64;

/// The columns of the list of a stream, and of the drawer opened from it.
pub fn default_columns(stream: RumStream) -> &'static [&'static str] {
    match stream {
        RumStream::Sessions => &[
            "session.id",
            "session.type",
            "synthetics.test_id",
            "synthetics.result_id",
            "session.has_replay",
            "session.last_activity",
            "session.duration",
            "session.view.count",
            "session.action.count",
            "session.error.count",
            "session.resource.count",
            "session.long_task.count",
            "session.frustration.count",
            "session.crash.count",
            "session.landing_view.url_path",
            "session.exit_view.url_path",
            "application.id",
            "service",
            "env",
            "version",
            "source",
            "usr.id",
            "usr.name",
            "usr.email",
            "usr.anonymous_id",
            "browser.name",
            "browser.version",
            "os.name",
            "os.version",
            "device.type",
            "device.model",
            "geo.country_iso_code",
            "geo.country",
            "geo.city",
            "network.client.ip",
        ],
        RumStream::Views => &[
            "view.id",
            "view.url",
            "view.url_path",
            "view.url_path_group",
            "view.name",
            "view.is_active",
            "view.loading_type",
            "view.loading_time",
            "view.time_spent",
            "view.first_byte",
            "view.performance.fcp.timestamp",
            "view.performance.lcp.timestamp",
            "view.performance.inp.duration",
            "view.performance.cls.score",
            "view.action.count",
            "view.error.count",
            "view.resource.count",
            "view.long_task.count",
            "view.frustration.count",
            "session.id",
            "application.id",
            "service",
            "env",
            "version",
            "source",
            "usr.id",
            "browser.name",
            "os.name",
            "device.type",
            "geo.country_iso_code",
        ],
        RumStream::Actions => &[
            "action.id",
            "action.type",
            "action.target.name",
            "action.target.selector",
            "action.loading_time",
            "action.frustration.type",
            "action.frustration.count",
            "action.error.count",
            "action.resource.count",
            "view.id",
            "view.url_path",
            "session.id",
            "application.id",
            "service",
            "env",
            "version",
            "usr.id",
            "browser.name",
            "device.type",
            "geo.country_iso_code",
        ],
        RumStream::Errors => &[
            "error.id",
            "error.message",
            "error.source",
            "error.type",
            "error.handling",
            "error.is_crash",
            "error.category",
            "error.issue_id",
            "error.resource.status_code",
            "error.resource.url",
            "action.id",
            "view.id",
            "view.url_path",
            "session.id",
            "application.id",
            "service",
            "env",
            "version",
            "usr.id",
            "browser.name",
            "device.type",
            "geo.country_iso_code",
        ],
        RumStream::Resources => &[
            "resource.id",
            "resource.type",
            "resource.method",
            "resource.url",
            "resource.url_host",
            "resource.url_path_group",
            "resource.status_code",
            "resource.duration",
            "resource.size",
            "resource.transfer_size",
            "resource.trace_id",
            "resource.span_id",
            "action.id",
            "view.id",
            "view.url_path",
            "session.id",
            "application.id",
            "service",
            "env",
            "version",
            "usr.id",
            "browser.name",
            "geo.country_iso_code",
        ],
        RumStream::LongTasks => &[
            "long_task.id",
            "long_task.duration",
            "long_task.entry_type",
            "long_task.start_time",
            "long_task.blocking_duration",
            "long_task.is_frozen_frame",
            "action.id",
            "view.id",
            "view.url_path",
            "session.id",
            "application.id",
            "service",
            "env",
            "version",
            "usr.id",
            "browser.name",
        ],
        RumStream::Vitals => &[
            "vital.id",
            "vital.type",
            "vital.name",
            "vital.duration",
            "vital.app_launch_metric",
            "vital.startup_type",
            "view.id",
            "view.url_path",
            "session.id",
            "application.id",
            "service",
            "env",
            "version",
            "usr.id",
        ],
        RumStream::Logs => &[
            "log.id",
            "level",
            "message",
            "log.logger",
            "log.origin",
            "log.error_type",
            "log.stack",
            "view.id",
            "view.url_path",
            "session.id",
            "application.id",
            "service",
            "env",
            "version",
            "usr.id",
            "browser.name",
        ],
    }
}

/// Columns the server keeps for itself: what a session carries from one row to the next.
const INTERNAL: &[&str] = &["session.view_state"];

pub fn is_internal(name: &str) -> bool {
    INTERNAL.contains(&name)
}

/// Every column of a stream a page may show, for the page of one row that shows it whole.
pub fn all_columns(stream: RumStream) -> Vec<&'static str> {
    fields(stream)
        .iter()
        .map(|column| column.name)
        .filter(|name| !is_internal(name))
        .collect()
}

/// The columns free text is looked for in.
pub fn text_columns(stream: RumStream) -> &'static [&'static str] {
    match stream {
        RumStream::Sessions => &[
            "session.id",
            "usr.id",
            "usr.name",
            "usr.email",
            "session.landing_view.url_path",
            "session.exit_view.url_path",
        ],
        RumStream::Views => &["view.url", "view.name", "view.id", "usr.id", "usr.email"],
        RumStream::Actions => &["action.target.name", "view.url_path", "usr.id"],
        RumStream::Errors => &["error.message", "error.type", "view.url_path", "usr.id"],
        RumStream::Resources => &["resource.url", "view.url_path"],
        RumStream::LongTasks => &["view.url_path", "usr.id"],
        RumStream::Vitals => &["vital.name", "view.url_path", "usr.id"],
        RumStream::Logs => &["message", "log.logger", "view.url_path", "usr.id"],
    }
}

/// What a bar of the histogram counts apart from the rest, as a condition and the columns it
/// reads: the sessions that had an error, the requests that failed.
pub fn trouble(stream: RumStream) -> Option<(&'static str, &'static [&'static str])> {
    match stream {
        RumStream::Sessions => Some((
            "COALESCE(\"session.error.count\", 0) > 0",
            &["session.error.count"],
        )),
        RumStream::Views => Some((
            "COALESCE(\"view.error.count\", 0) > 0",
            &["view.error.count"],
        )),
        RumStream::Actions => Some((
            "COALESCE(\"action.frustration.count\", 0) > 0",
            &["action.frustration.count"],
        )),
        RumStream::Resources => Some((
            "(\"resource.status_code\" >= 400 OR \"resource.status_code\" = 0)",
            &["resource.status_code"],
        )),
        RumStream::Logs => Some(("\"level\" = 'error'", &["level"])),
        RumStream::Errors | RumStream::LongTasks | RumStream::Vitals => None,
    }
}

/// The columns a request asks for, checked against the stream. None asked: the defaults. The
/// column that names the row is always read: a page opens a row by it.
pub fn requested_columns(stream: RumStream, asked: &[String]) -> Result<Vec<&'static str>> {
    if asked.is_empty() {
        return Ok(default_columns(stream).to_vec());
    }

    if asked.len() > MAX_COLUMNS {
        return Err(Error::invalid("too many columns"));
    }

    let mut columns = asked
        .iter()
        .map(|name| {
            field(stream, name.trim())
                .map(|column| column.name)
                .ok_or_else(|| {
                    Error::invalid(format!("`{name}` is not a field of {}", stream.name()))
                })
        })
        .collect::<Result<Vec<_>>>()?;

    if !columns.contains(&stream.id_field()) {
        columns.push(stream.id_field());
    }

    Ok(columns)
}

/// What a row says of the session it belongs to, which a timeline of that session does not repeat.
const SESSION_DIMENSIONS: &[&str] = &[
    "application.id",
    "service",
    "env",
    "version",
    "source",
    "usr.",
    "browser.",
    "os.",
    "device.",
    "geo.",
    "network.",
];

/// The columns of a row in the timeline of one session: the defaults without the session's own
/// dimensions, which the session says once.
pub fn timeline_columns(stream: RumStream) -> Vec<&'static str> {
    default_columns(stream)
        .iter()
        .copied()
        .filter(|name| {
            !SESSION_DIMENSIONS.iter().any(|dimension| {
                if dimension.ends_with('.') {
                    name.starts_with(dimension)
                } else {
                    name == dimension
                }
            })
        })
        .collect()
}

/// Whether a column can be counted by value: a JSON object has no values to count.
pub fn is_facetable(stream: RumStream, name: &str) -> bool {
    field(stream, name).is_some_and(|column| column.data_type != FieldType::Json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_and_text_column_is_a_column_of_its_stream() {
        for stream in RumStream::ALL {
            for name in default_columns(stream).iter().chain(text_columns(stream)) {
                assert!(field(stream, name).is_some(), "{name} in {stream:?}");
            }

            if let Some((_, columns)) = trouble(stream) {
                for name in columns {
                    assert!(field(stream, name).is_some(), "{name} in {stream:?}");
                }
            }
        }
    }

    #[test]
    fn text_search_never_reads_a_json_column() {
        for stream in RumStream::ALL {
            for name in text_columns(stream) {
                assert_ne!(
                    field(stream, name).unwrap().data_type,
                    FieldType::Json,
                    "{name}"
                );
            }
        }
    }

    #[test]
    fn asked_columns_are_checked_and_none_asked_gives_the_defaults() {
        assert_eq!(
            requested_columns(RumStream::Views, &[]).unwrap(),
            default_columns(RumStream::Views)
        );
        assert_eq!(
            requested_columns(RumStream::Views, &["context".into(), " env ".into()]).unwrap(),
            vec!["context", "env", "view.id"]
        );
        assert!(requested_columns(RumStream::Views, &["nope".into()]).is_err());
        assert!(requested_columns(RumStream::Views, &vec!["env".to_string(); 65]).is_err());
    }

    #[test]
    fn a_row_is_shown_whole_without_what_the_server_keeps_for_itself() {
        let columns = all_columns(RumStream::Sessions);

        assert!(columns.contains(&"session.error.count"));
        assert!(!columns.contains(&"session.view_state"));
        assert!(all_columns(RumStream::Views).contains(&"context"));
    }

    #[test]
    fn the_column_that_names_a_row_is_always_read() {
        assert_eq!(
            requested_columns(RumStream::Errors, &["error.message".into()]).unwrap(),
            vec!["error.message", "error.id"]
        );
        assert_eq!(
            requested_columns(RumStream::Errors, &["error.id".into()]).unwrap(),
            vec!["error.id"]
        );
    }

    #[test]
    fn a_timeline_leaves_out_what_the_session_says_once() {
        let columns = timeline_columns(RumStream::Resources);

        assert!(columns.contains(&"resource.url"));
        assert!(columns.contains(&"view.id"));
        assert!(!columns.contains(&"browser.name"));
        assert!(!columns.contains(&"geo.country_iso_code"));
        assert!(!columns.contains(&"service"));
        assert!(!columns.contains(&"usr.id"));

        for stream in RumStream::ALL {
            assert!(
                timeline_columns(stream).contains(&stream.id_field()),
                "{stream:?}"
            );
        }
    }

    #[test]
    fn json_columns_cannot_be_counted_by_value() {
        assert!(is_facetable(RumStream::Views, "env"));
        assert!(is_facetable(RumStream::Views, "view.error.count"));
        assert!(!is_facetable(RumStream::Views, "context"));
        assert!(!is_facetable(RumStream::Views, "nope"));
    }
}
