// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a field holds across the log rows a structured query matches.
//!
//! The list returns a page, and the explorer's field list counts a field among
//! the rows of that page. This reads the field in every row the same query
//! matches in its time range, selecting with the conditions the list itself
//! selects by.

use axum::{Extension, Json, extract::State};
use serde::Deserialize;

use super::{
    LogFilter, clean_required, match_clauses, normalize_filters, normalize_free_text,
    validate_filter_fields,
};
use crate::{
    api::{
        AppState,
        http::routes::web::field_stats::{
            Analysed, FieldKind, FieldStatsResponse, Scope, Unavailable, analyze,
        },
    },
    app::iam::IamContext,
    domain::{
        iam::permission,
        intake::EVENT_ID_FIELD,
        stream::{FieldDef, FieldType, StreamType},
    },
    shared::{Error, Result},
};

#[derive(Debug, Default, Deserialize)]
pub(super) struct LogFieldStatsRequest {
    stream: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    #[serde(default)]
    filters: Vec<LogFilter>,
    #[serde(default)]
    free_text: Vec<String>,
    field: Option<String>,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
pub(super) async fn field_stats(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Json(request): Json<LogFieldStatsRequest>,
) -> Result<Json<FieldStatsResponse>> {
    let (Some(from), Some(to)) = (request.from, request.to) else {
        return Err(Error::invalid("field analysis range is required"));
    };

    if to <= from {
        return Err(Error::invalid(
            "field analysis range end must be greater than start",
        ));
    }

    let stream_name = clean_required(request.stream, "log stream is required", 128)?;
    let field_name = clean_required(request.field, "log field is required", 128)?;
    let filters = normalize_filters(request.filters)?;
    let free_text = normalize_free_text(request.free_text)?;

    let stream = match state
        .telemetry
        .streams
        .get(&iam.org_id, &stream_name, StreamType::LOGS)
        .await
    {
        Ok(stream) => stream,
        // A stream nothing was written to matches nothing: its list is empty too.
        Err(Error::NotFound(_)) => return Ok(Json(FieldStatsResponse::empty(FieldKind::Text))),
        Err(error) => return Err(error),
    };

    let fields = &stream.schema.fields;
    validate_filter_fields(&filters, &free_text, fields)?;

    let (field_type, encrypted) = declared(&field_name, fields)
        .ok_or_else(|| Error::invalid(format!("unknown log field: {field_name}")))?;

    let field = Analysed::new(&field_name, field_type)?;

    // An encrypted field is stored as ciphertext: its counts would be of that.
    if encrypted {
        return Ok(Json(FieldStatsResponse::unavailable(
            field.kind,
            Unavailable::FieldProtected,
        )));
    }

    let scope = Scope {
        stream: stream_name,
        stream_type: StreamType::LOGS,
        from,
        to,
        clauses: match_clauses((from, to), &filters, &free_text, fields)?,
    };

    Ok(Json(analyze(&state, &iam, &scope, &field).await?))
}

/// The type a field is declared with and whether it is encrypted at rest. The
/// two fields every log row has are not in the schema of a stream.
fn declared(name: &str, fields: &[FieldDef]) -> Option<(FieldType, bool)> {
    match name {
        "_timestamp" => Some((FieldType::Timestamp, false)),
        EVENT_ID_FIELD => Some((FieldType::Utf8, false)),
        _ => fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| (field.data_type, field.encrypted)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, data_type: FieldType, encrypted: bool) -> FieldDef {
        FieldDef {
            name: name.into(),
            data_type,
            nullable: true,
            index_type: None,
            indexed: false,
            encrypted,
            exact: false,
        }
    }

    #[test]
    fn a_field_is_read_as_the_stream_declares_it() {
        let fields = [
            field("status", FieldType::Int64, false),
            field("token", FieldType::Utf8, true),
        ];

        assert_eq!(declared("status", &fields), Some((FieldType::Int64, false)));
        assert_eq!(declared("token", &fields), Some((FieldType::Utf8, true)));
        assert_eq!(
            declared("_timestamp", &fields),
            Some((FieldType::Timestamp, false))
        );
        assert_eq!(declared("missing", &fields), None);
    }
}
