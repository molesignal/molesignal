// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/rum/analytics/{funnel,paths,retention}`: product analytics over the sessions of an
//! application.
//!
//! - `funnel` counts the sessions that did a series of steps in order, and the time between steps.
//! - `paths` follows the pages sessions opened after a page, or before one.
//! - `retention` counts who came back, by the period they were first seen in.
//!
//! Each is asked with `GET` (the steps and the filters are JSON lists in their parameters, so a
//! question is an address) or `POST`, and answers the same.

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::Deserialize;

use super::{address::json_list, reader::Reader};
use crate::{
    api::AppState,
    app::{
        iam::IamContext,
        rum::analytics::{self, Answer, FunnelRequest, PathsRequest, RetentionRequest},
    },
    domain::iam::permission,
    infra::rum::{
        analytics::{Funnel, Paths, Period, Retention, Step},
        query::Filter,
    },
    shared::Result,
};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FunnelQuery {
    from: Option<i64>,
    to: Option<i64>,
    #[serde(default, deserialize_with = "json_list")]
    filters: Vec<Filter>,
    #[serde(default, deserialize_with = "json_list")]
    steps: Vec<Step>,
    window_secs: Option<i64>,
    breakdown: Option<String>,
}

impl From<FunnelQuery> for FunnelRequest {
    fn from(query: FunnelQuery) -> Self {
        Self {
            from: query.from,
            to: query.to,
            filters: query.filters,
            steps: query.steps,
            window_secs: query.window_secs,
            breakdown: query.breakdown,
        }
    }
}

#[permission(any("rum.analytics.read", "sys.telemetry.read"))]
pub(super) async fn funnel_post(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Json(request): Json<FunnelRequest>,
) -> Result<Json<Answer<Funnel>>> {
    let reader = Reader::new(&state, &iam);

    analytics::funnel(&reader, &request).await.map(Json)
}

#[permission(any("rum.analytics.read", "sys.telemetry.read"))]
pub(super) async fn funnel_get(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(query): Query<FunnelQuery>,
) -> Result<Json<Answer<Funnel>>> {
    let reader = Reader::new(&state, &iam);

    analytics::funnel(&reader, &query.into()).await.map(Json)
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct PathsQuery {
    from: Option<i64>,
    to: Option<i64>,
    #[serde(default, deserialize_with = "json_list")]
    filters: Vec<Filter>,
    anchor: Option<String>,
    #[serde(default)]
    backward: bool,
    depth: Option<usize>,
    by: Option<String>,
}

impl From<PathsQuery> for PathsRequest {
    fn from(query: PathsQuery) -> Self {
        Self {
            from: query.from,
            to: query.to,
            filters: query.filters,
            anchor: query.anchor,
            backward: query.backward,
            depth: query.depth,
            by: query.by,
        }
    }
}

#[permission(any("rum.analytics.read", "sys.telemetry.read"))]
pub(super) async fn paths_post(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Json(request): Json<PathsRequest>,
) -> Result<Json<Answer<Paths>>> {
    let reader = Reader::new(&state, &iam);

    analytics::paths(&reader, &request).await.map(Json)
}

#[permission(any("rum.analytics.read", "sys.telemetry.read"))]
pub(super) async fn paths_get(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(query): Query<PathsQuery>,
) -> Result<Json<Answer<Paths>>> {
    let reader = Reader::new(&state, &iam);

    analytics::paths(&reader, &query.into()).await.map(Json)
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct RetentionQuery {
    from: Option<i64>,
    to: Option<i64>,
    #[serde(default, deserialize_with = "json_list")]
    filters: Vec<Filter>,
    period: Option<Period>,
    periods: Option<usize>,
}

impl From<RetentionQuery> for RetentionRequest {
    fn from(query: RetentionQuery) -> Self {
        Self {
            from: query.from,
            to: query.to,
            filters: query.filters,
            period: query.period,
            periods: query.periods,
        }
    }
}

#[permission(any("rum.analytics.read", "sys.telemetry.read"))]
pub(super) async fn retention_post(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Json(request): Json<RetentionRequest>,
) -> Result<Json<Answer<Retention>>> {
    let reader = Reader::new(&state, &iam);

    analytics::retention(&reader, &request).await.map(Json)
}

#[permission(any("rum.analytics.read", "sys.telemetry.read"))]
pub(super) async fn retention_get(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(query): Query<RetentionQuery>,
) -> Result<Json<Answer<Retention>>> {
    let reader = Reader::new(&state, &iam);

    analytics::retention(&reader, &query.into()).await.map(Json)
}

#[cfg(test)]
mod tests {
    use axum::http::Uri;

    use super::*;

    fn from_address(query: &str) -> std::result::Result<FunnelQuery, String> {
        let uri: Uri = format!("http://localhost/api/v1/rum/analytics/funnel?{query}")
            .parse()
            .expect("uri");

        Query::<FunnelQuery>::try_from_uri(&uri)
            .map(|query| query.0)
            .map_err(|rejection| rejection.body_text())
    }

    fn escaped(text: &str) -> String {
        text.bytes()
            .map(|byte| {
                if byte.is_ascii_alphanumeric() {
                    char::from(byte).to_string()
                } else {
                    format!("%{byte:02X}")
                }
            })
            .collect()
    }

    #[test]
    fn an_address_says_what_a_body_says() {
        let steps = r#"[{"type":"views","filters":[{"field":"view.url_path_group","op":"=","value":"/"}]},{"type":"actions","name":"Pay","filters":[]}]"#;

        let address = from_address(&format!(
            "steps={}&windowSecs=1800&breakdown=version",
            escaped(steps)
        ))
        .expect("address");

        let body: FunnelRequest = serde_json::from_value(serde_json::json!({
            "steps": [
                {
                    "type": "views",
                    "filters": [{ "field": "view.url_path_group", "op": "=", "value": "/" }],
                },
                { "type": "actions", "name": "Pay", "filters": [] },
            ],
            "windowSecs": 1800,
            "breakdown": "version",
        }))
        .expect("body");

        let request: FunnelRequest = address.into();

        assert_eq!(request.steps, body.steps);
        assert_eq!(request.window_secs, Some(1800));
        assert_eq!(request.breakdown.as_deref(), Some("version"));
    }

    #[test]
    fn steps_that_are_not_a_list_are_refused() {
        assert!(from_address("steps=views").is_err());
        assert!(from_address("steps=").is_ok());
    }
}
