// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET /api/v1/query`: the query of `POST /api/v1/query` said in the query
//! string, so a read is an address that can be copied and opened again. The
//! answer, the headers it honours and the permission are those of the `POST`;
//! a statement too long for an address is still sent as a `POST` body.

use axum::{
    Extension, Json,
    extract::{Query, State},
    http::HeaderMap,
    response::Response,
};
use serde::Deserialize;

use super::{ClustersParam, execute_query};
use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::permission,
        query::{QueryLanguage, QueryRequest, StreamHint},
        stream::StreamType,
    },
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

/// The fields of a query body, flat: `timeRange` as `start` and `end`, the
/// stream hint as `stream` and `streamType`, and the federated clusters as
/// the `POST` takes them in its own query string.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct QueryAddress {
    language: QueryLanguage,
    statement: String,
    /// Epoch microseconds.
    start: i64,
    /// Epoch microseconds.
    end: i64,
    stream: Option<String>,
    stream_type: Option<StreamType>,
    limit: Option<usize>,
    /// PromQL range step in microseconds, the unit of `start` and `end`.
    step: Option<i64>,
    clusters: Option<String>,
}

impl QueryAddress {
    /// The body this address stands for. A stream is named with its type, as
    /// the body's stream hint has both.
    fn into_request(self, org_id: Id) -> Result<(QueryRequest, ClustersParam)> {
        let stream = match (self.stream, self.stream_type) {
            (Some(name), Some(stream_type)) => Some(StreamHint { name, stream_type }),
            (None, None) => None,
            _ => {
                return Err(Error::invalid(
                    "`stream` and `streamType` are given together",
                ));
            }
        };

        let request = QueryRequest {
            org_id,
            language: self.language,
            statement: self.statement,
            time_range: TimeRange::new(TimestampMicros(self.start), TimestampMicros(self.end)),
            stream,
            limit: self.limit,
            step: self.step,
            federation_clusters: Vec::new(),
        };

        Ok((
            request,
            ClustersParam {
                clusters: self.clusters,
            },
        ))
    }
}

#[permission(any("streams.query", "sys.telemetry.read"))]
pub(super) async fn execute(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    headers: HeaderMap,
    Query(address): Query<QueryAddress>,
) -> Result<Response> {
    let (request, clusters) = address.into_request(ctx.org_id.clone())?;

    execute_query(
        State(state),
        Extension(ctx),
        Query(clusters),
        headers,
        Json(request),
    )
    .await
}

#[cfg(test)]
mod tests {
    use axum::http::Uri;

    use super::*;

    fn address(query: &str) -> QueryAddress {
        let uri: Uri = format!("http://localhost/api/v1/query?{query}")
            .parse()
            .expect("uri");

        Query::<QueryAddress>::try_from_uri(&uri)
            .expect("query address")
            .0
    }

    #[test]
    fn an_address_says_what_a_body_says() {
        let (request, clusters) = address(
            "language=promql&statement=sum%28rate%28http_requests_total%5B5m%5D%29%29\
             &start=1000&end=2000&limit=50&step=100&clusters=local,eu",
        )
        .into_request(Id::from_string("org"))
        .expect("request");

        assert_eq!(request.language, QueryLanguage::Promql);
        assert_eq!(request.statement, "sum(rate(http_requests_total[5m]))");
        assert_eq!(
            (request.time_range.start, request.time_range.end),
            (TimestampMicros(1000), TimestampMicros(2000))
        );
        assert_eq!(request.limit, Some(50));
        assert_eq!(request.step, Some(100));
        assert!(request.stream.is_none());
        assert_eq!(clusters.clusters.as_deref(), Some("local,eu"));
    }

    #[test]
    fn a_stream_is_named_with_its_type() {
        let (request, _) =
            address("language=sql&statement=SELECT%201&start=0&end=1&stream=app&streamType=logs")
                .into_request(Id::from_string("org"))
                .expect("request");
        let stream = request.stream.expect("stream hint");

        assert_eq!(
            (stream.name.as_str(), stream.stream_type),
            ("app", StreamType::LOGS)
        );

        let alone = address("language=sql&statement=SELECT%201&start=0&end=1&stream=app")
            .into_request(Id::from_string("org"));

        assert!(alone.is_err());
    }
}
