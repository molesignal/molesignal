// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET /api/v1/synthetics/availability`: how the checks did over a time range, read from the
//! `_synthetic_results` stream. An organization whose checks never ran has no stream yet, and
//! gets a range nothing ran in.

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::Deserialize;

use crate::{
    api::AppState,
    app::{
        iam::IamContext,
        synthetics::{
            RESULT_STREAM,
            availability::{
                Availability, AvailabilityQuery, Bucket, CheckSummary, LocationBucket,
                LocationSummary, bucket_start, summaries, text,
            },
        },
    },
    domain::{
        iam::permission,
        query::{QueryLanguage, QueryRequest, QueryResult, StreamHint},
        stream::StreamType,
    },
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

const DAY: i64 = 24 * 60 * 60 * 1_000_000;

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/synthetics/availability", get(availability))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AvailabilityParams {
    /// The start of the range, in microseconds; a day before its end when left out.
    from: Option<i64>,
    /// The end of the range; now when left out.
    to: Option<i64>,
    monitor_id: Option<String>,
    location_id: Option<String>,
    /// The step the range is cut into, in seconds; chosen from the range when left out.
    step: Option<u32>,
}

#[permission("synthetics.results.read")]
async fn availability(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Query(params): Query<AvailabilityParams>,
) -> Result<Json<Availability>> {
    let to = params.to.unwrap_or_else(|| TimestampMicros::now().0);
    let from = params.from.unwrap_or(to.saturating_sub(DAY));
    let id = |value: Option<String>| value.filter(|value| !value.trim().is_empty()).map(Id);

    let stream = state
        .telemetry
        .streams
        .get(&context.org_id, RESULT_STREAM, StreamType::LOGS)
        .await;

    // A column only exists once a run has written it: before any run took time, there is none.
    let durations = match &stream {
        Ok(definition) => definition
            .schema
            .fields
            .iter()
            .any(|field| field.name == "duration_ms"),
        Err(_) => false,
    };

    let query = AvailabilityQuery::new(
        from,
        to,
        params.step,
        id(params.monitor_id),
        id(params.location_id),
        durations,
    )?;

    match stream {
        Ok(_) => {}
        Err(Error::NotFound(_)) => return Ok(Json(Availability::empty(&query))),
        Err(error) => return Err(error),
    }

    let run = |statement: String| run_sql(&state, &context, &query, statement);

    let total = run(query.total_sql()).await?;
    let checks = run(query.checks_sql()).await?;
    let locations = run(query.locations_sql()).await?;
    let buckets = run(query.buckets_sql()).await?;

    let location_buckets = match query.location_buckets_sql() {
        Some(statement) => Some(run(statement).await?),
        None => None,
    };

    Ok(Json(Availability {
        from: query.from,
        to: query.to,
        step: query.step,
        total: summaries(&total, &[])
            .into_iter()
            .next()
            .map(|(_, summary)| summary)
            .unwrap_or_default(),
        checks: summaries(&checks, &["check_id"])
            .into_iter()
            .map(|(keys, summary)| CheckSummary {
                check_id: text(&keys[0]),
                summary,
            })
            .collect(),
        locations: summaries(&locations, &["location_id"])
            .into_iter()
            .map(|(keys, summary)| LocationSummary {
                location_id: text(&keys[0]),
                summary,
            })
            .collect(),
        buckets: summaries(&buckets, &["bucket"])
            .into_iter()
            .map(|(keys, summary)| Bucket {
                start: bucket_start(&query, &keys[0]),
                summary,
            })
            .collect(),
        location_buckets: location_buckets
            .map(|result| {
                summaries(&result, &["location_id", "bucket"])
                    .into_iter()
                    .map(|(keys, summary)| LocationBucket {
                        location_id: text(&keys[0]),
                        start: bucket_start(&query, &keys[1]),
                        summary,
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }))
}

async fn run_sql(
    state: &AppState,
    context: &IamContext,
    query: &AvailabilityQuery,
    statement: String,
) -> Result<QueryResult> {
    state
        .query
        .run(QueryRequest {
            org_id: context.org_id.clone(),
            language: QueryLanguage::Sql,
            statement,
            time_range: TimeRange::new(TimestampMicros(query.from), TimestampMicros(query.to)),
            // The runs are written as logs (see the `synthetic_results` worker) and read so.
            stream: Some(StreamHint {
                name: RESULT_STREAM.to_string(),
                stream_type: StreamType::LOGS,
            }),
            limit: None,
            step: None,
            federation_clusters: Vec::new(),
        })
        .await
}
