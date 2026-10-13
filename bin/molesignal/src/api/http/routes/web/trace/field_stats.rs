// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a field holds across the Spans of the Traces an explorer has listed.
//!
//! The list returns a row for each Trace, which says nothing of what its
//! Spans hold. A field of a Span is therefore read from the Spans of the
//! listed Traces themselves, named by their ids: the numbers are of exactly
//! the Traces on the page, every Span of them.

use axum::{Extension, Json, extract::State};
use serde::Deserialize;

use super::{
    list::MAX_TRACE_PAGE_SIZE, request::validate_trace_id, resolve_traces_stream_definition,
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
        stream::{StreamDefinition, StreamType},
    },
    shared::{Error, Result},
};

#[derive(Debug, Default, Deserialize)]
pub(super) struct TraceFieldStatsRequest {
    /// The stream the Traces were listed from. Without one, the stream the
    /// list itself reads.
    stream: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    #[serde(default)]
    trace_ids: Vec<String>,
    field: Option<String>,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
pub(super) async fn field_stats(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<TraceFieldStatsRequest>,
) -> Result<Json<FieldStatsResponse>> {
    let (Some(from), Some(to)) = (request.from, request.to) else {
        return Err(Error::invalid("field analysis range is required"));
    };

    if to <= from {
        return Err(Error::invalid(
            "field analysis range end must be greater than start",
        ));
    }

    let field_name = request.field.as_deref().map(str::trim).unwrap_or_default();

    if field_name.is_empty() || field_name.len() > 128 {
        return Err(Error::invalid("trace field is required"));
    }

    let trace_ids = listed_traces(request.trace_ids)?;

    let Some(definition) = stream_definition(&state, &ctx, request.stream.as_deref()).await? else {
        return Ok(Json(FieldStatsResponse::empty(FieldKind::Text)));
    };

    let declared = definition
        .schema
        .fields
        .iter()
        .find(|field| field.name == field_name)
        .ok_or_else(|| Error::invalid(format!("unknown trace field: {field_name}")))?;

    let field = Analysed::new(field_name, declared.data_type)?;

    // An encrypted field is stored as ciphertext: its counts would be of that.
    if declared.encrypted {
        return Ok(Json(FieldStatsResponse::unavailable(
            field.kind,
            Unavailable::FieldProtected,
        )));
    }

    if trace_ids.is_empty() {
        return Ok(Json(FieldStatsResponse::empty(field.kind)));
    }

    let scope = Scope {
        stream: definition.name.clone(),
        stream_type: StreamType::TRACES,
        from,
        to,
        clauses: vec![format!("trace_id IN ({})", trace_ids.join(", "))],
    };

    Ok(Json(analyze(&state, &ctx, &scope, &field).await?))
}

/// The ids of the listed Traces as SQL literals, each once. An id is made of
/// letters, digits, `-` and `_` only, so it is written between quotes as it is.
fn listed_traces(ids: Vec<String>) -> Result<Vec<String>> {
    if ids.len() > MAX_TRACE_PAGE_SIZE {
        return Err(Error::invalid("too many traces to analyse a field of"));
    }

    let mut literals = Vec::with_capacity(ids.len());

    for id in ids {
        validate_trace_id(&id).map_err(Error::invalid)?;

        let literal = format!("'{id}'");

        if !literals.contains(&literal) {
            literals.push(literal);
        }
    }

    Ok(literals)
}

/// The traces stream the explorer listed from, or the one the list reads when
/// none is named. `None` when the organization has no such stream.
async fn stream_definition(
    state: &AppState,
    ctx: &IamContext,
    named: Option<&str>,
) -> Result<Option<StreamDefinition>> {
    let Some(name) = named.map(str::trim).filter(|name| !name.is_empty()) else {
        return Ok(resolve_traces_stream_definition(state, &ctx.org_id).await);
    };

    if name.len() > 128 {
        return Err(Error::invalid("trace stream name is too long"));
    }

    match state
        .telemetry
        .streams
        .get(&ctx.org_id, name, StreamType::TRACES)
        .await
    {
        Ok(stream) => Ok(Some(stream)),
        Err(Error::NotFound(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listed_traces_are_quoted_once_each() {
        let ids = ["abc-1", "def_2", "abc-1"].map(String::from).to_vec();

        assert_eq!(listed_traces(ids).expect("ids"), ["'abc-1'", "'def_2'"]);
    }

    #[test]
    fn an_id_that_could_end_the_literal_is_refused() {
        assert!(listed_traces(vec!["a' OR '1'='1".into()]).is_err());
        assert!(listed_traces(vec![String::new()]).is_err());
    }

    #[test]
    fn no_more_traces_than_a_page_holds_are_read() {
        let ids = (0..=MAX_TRACE_PAGE_SIZE)
            .map(|index| format!("t{index}"))
            .collect();

        assert!(listed_traces(ids).is_err());
    }
}
