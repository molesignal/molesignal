// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The rows the intake makes fit the fixed schemas of their streams.
//!
//! The store refuses a row whose value has the wrong type, and a column that no schema declares
//! is added to the schema when it is first seen, so the fixed schemas would stop being fixed. The
//! events are the valid fixtures of the contract, which every SDK shares, stamped as a request from
//! a browser in Berlin would stamp them.

use std::{collections::HashSet, fs, path::PathBuf, sync::Arc};

use async_trait::async_trait;
use serde_json::{Map, Value, json};

use super::{
    enrich::{AgentInfo, Origin},
    flatten::flatten,
    validate::validate,
};
use crate::{
    app::intake::check_event_types,
    domain::{
        intake::RawEvent,
        rum::{
            RumApplicationSettings, RumSessionMerger,
            event::{RumStream, stream_schema},
        },
    },
    infra::{
        enrichment::GeoLocation,
        rum::session_state::{SessionHistory, SessionMerger},
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

const NOW: i64 = 1_760_000_100_000;

fn origin() -> Origin {
    Origin {
        ip: Some("203.0.113.7".parse().expect("an address")),
        place: Some(GeoLocation {
            country: Some("DE".into()),
            country_name: Some("Germany".into()),
            continent_code: Some("EU".into()),
            continent: Some("Europe".into()),
            region: Some("Berlin".into()),
            city: Some("Berlin".into()),
            latitude: Some(52.52),
            longitude: Some(13.405),
        }),
        agent: Some(Arc::new(AgentInfo {
            browser_name: Some("Chrome".into()),
            browser_version: Some("130.0.0.0".into()),
            os_name: Some("macOS".into()),
            os_version: Some("10.15.7".into()),
            device_type: Some("desktop"),
        })),
    }
}

/// The events of the valid fixtures, each with the name of its file.
fn fixtures() -> Vec<(String, Value)> {
    let directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contracts/rum/events/fixtures/valid");

    let mut files = fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect::<Vec<_>>();

    files.sort();

    files
        .into_iter()
        .flat_map(|path| {
            let name = path
                .file_name()
                .expect("a file name")
                .to_string_lossy()
                .into_owned();

            let text = fs::read_to_string(&path).expect("a readable fixture");
            let body: Value = serde_json::from_str(&text).expect("a JSON fixture");

            match body {
                Value::Array(events) => events
                    .into_iter()
                    .map(|event| (name.clone(), event))
                    .collect::<Vec<_>>(),
                event => vec![(name, event)],
            }
        })
        .collect()
}

/// The row each event becomes, with the stream it is written to.
fn rows() -> Vec<(String, RumStream, RawEvent)> {
    fixtures()
        .into_iter()
        .map(|(name, event)| {
            let validated =
                validate(event, NOW).unwrap_or_else(|reason| panic!("{name}: {reason}"));
            let stream = validated.kind.stream();

            let mut fields = validated.event;

            fields.insert("application".into(), json!({ "id": "shop-web" }));

            let mut row = flatten(stream, fields);

            origin().stamp(&mut row, &RumApplicationSettings::default());

            let event = RawEvent {
                timestamp: TimestampMicros(validated.date_ms * 1000),
                fields: row,
            };

            (name, stream, event)
        })
        .collect()
}

fn assert_fits(name: &str, stream: RumStream, event: &RawEvent) {
    let schema = stream_schema(stream);

    let declared = schema
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<HashSet<_>>();

    for column in event.fields.keys() {
        assert!(
            declared.contains(column.as_str()),
            "{name}: `{column}` is no column of {}",
            stream.name()
        );
    }

    if let Err(reason) = check_event_types(&schema, event) {
        panic!("{name}: {reason}");
    }
}

#[test]
fn every_event_of_the_contract_becomes_a_row_that_fits_its_stream() {
    let rows = rows();

    for (name, stream, event) in &rows {
        assert_fits(name, *stream, event);
    }

    // Not vacuous: the fixtures hold an event of every type but the one the server derives.
    let streams = rows
        .iter()
        .map(|(_, stream, _)| *stream)
        .collect::<HashSet<_>>();

    for stream in RumStream::ALL {
        assert_eq!(
            streams.contains(&stream),
            stream != RumStream::Sessions,
            "{stream:?}"
        );
    }
}

struct NoHistory;

#[async_trait]
impl SessionHistory for NoHistory {
    async fn rows(
        &self,
        _org_id: &Id,
        _session_id: &str,
        _from_ms: i64,
        _to_ms: i64,
    ) -> Result<Vec<Map<String, Value>>> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn the_sessions_built_from_those_views_fit_their_stream_too() {
    let patches = rows()
        .into_iter()
        .filter(|(_, stream, _)| *stream == RumStream::Views)
        .map(|(_, _, event)| event)
        .collect::<Vec<_>>();

    assert!(!patches.is_empty());

    let sessions = SessionMerger::new(Arc::new(NoHistory))
        .merge(&Id::from_string("org-1"), patches)
        .await
        .expect("sessions");

    assert!(!sessions.is_empty());

    for session in &sessions {
        assert_fits("a session", RumStream::Sessions, session);
    }
}
