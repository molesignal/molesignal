// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The indexes the host events are asked for by: an exact index on the host, the container, the
//! event, its domain and its severity (the OpenTelemetry dimensions of
//! [`crate::app::intake::LOG_DIMENSIONS`]).
//!
//! A log stream created by this build gives these fields an exact index when they first arrive,
//! so a new `host-events` stream needs nothing. A stream that received them before has them
//! without one; applying the recommendation gives each such field a rule, as the stream settings
//! page does, and the indexes of the data already written are rebuilt in the background. A field
//! with a rule of its own, enabled or not, is the organization's choice and is left as it is.
//! Partitions are not recommended: the agent rate-limits its events, and a stream this small is
//! read faster from fewer files.

use domain::stream::{
    FieldIndexRule, Schema, StreamIndexType, StreamRepository, StreamSettings, StreamType,
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::Serialize;

use super::EVENTS_STREAM;
use crate::app::intake::LOG_DIMENSIONS;

/// Where the indexes of the host events stand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state", content = "fields")]
pub enum IndexState {
    /// No event has arrived yet: the stream will be created with the indexes.
    NoStream,
    /// These fields arrived before and have no index.
    Missing(Vec<&'static str>),
    Ready,
}

/// The recommended fields of the stream that have neither an index nor a rule.
pub fn missing(schema: &Schema, settings: &StreamSettings) -> Vec<&'static str> {
    LOG_DIMENSIONS
        .into_iter()
        .filter(|name| {
            let ruled = settings.index_rules.iter().any(|rule| rule.field == *name);

            let unindexed = schema
                .fields
                .iter()
                .any(|field| field.name == *name && field.index_type == StreamIndexType::None);

            unindexed && !ruled
        })
        .collect()
}

/// The schema and the settings with an exact index on every field [`missing`] names; `None` when
/// none is missing. The time the indexes changed is `now`, so the files written before are
/// indexed again.
pub fn recommend(
    schema: &Schema,
    settings: &StreamSettings,
    now: TimestampMicros,
) -> Option<(Schema, StreamSettings)> {
    let missing = missing(schema, settings);

    if missing.is_empty() {
        return None;
    }

    let mut schema = schema.clone();
    let mut settings = settings.clone();

    for name in missing {
        settings.index_rules.push(FieldIndexRule {
            field: name.to_string(),
            enabled: true,
            index_type: StreamIndexType::Exact,
        });

        if let Some(field) = schema.fields.iter_mut().find(|field| field.name == name) {
            field.index_type = StreamIndexType::Exact;
        }
    }

    settings.index_changed_at = Some(now.0);

    Some((schema, settings))
}

/// Where the indexes of the organization's host events stand.
pub async fn state(streams: &dyn StreamRepository, org: &Id) -> Result<IndexState> {
    let stream = match streams.get(org, EVENTS_STREAM, StreamType::LOGS).await {
        Ok(stream) => stream,
        Err(Error::NotFound(_)) => return Ok(IndexState::NoStream),
        Err(error) => return Err(error),
    };

    let settings = streams.get_settings(&stream.id).await?;
    let missing = missing(&stream.schema, &settings);

    Ok(match missing.is_empty() {
        true => IndexState::Ready,
        false => IndexState::Missing(missing),
    })
}

/// Gives the host events the recommended indexes, and answers where they then stand.
pub async fn apply(streams: &dyn StreamRepository, org: &Id) -> Result<IndexState> {
    let stream = match streams.get(org, EVENTS_STREAM, StreamType::LOGS).await {
        Ok(stream) => stream,
        Err(Error::NotFound(_)) => return Ok(IndexState::NoStream),
        Err(error) => return Err(error),
    };

    let settings = streams.get_settings(&stream.id).await?;

    if let Some((schema, settings)) = recommend(&stream.schema, &settings, TimestampMicros::now()) {
        streams.update_schema(&stream.id, schema).await?;
        streams.update_settings(&stream.id, settings).await?;
    }

    Ok(IndexState::Ready)
}
