// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET|POST /rum/aggregate`: numbers over the rows the explorer's query matches, for each group
//! of up to two fields and, when asked, for each bar of time.
//!
//! The list returns a page, and the facets the commonest values of one field. This counts, sums,
//! averages and ranks across everything the same query matches in its range, so the overview and
//! the charts, tables and maps of the explorer agree with the list they sit beside.

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::Deserialize;

use super::{address::json_list, reader::Reader, selection::Selection};
use crate::{
    api::AppState,
    app::{
        iam::IamContext,
        rum::aggregate::{self, descending},
    },
    domain::iam::permission,
    infra::rum::query::{Aggregate, Filter, MeasureSpec},
    shared::Result,
};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AggregateRequest {
    #[serde(rename = "type")]
    kind: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    #[serde(default, deserialize_with = "json_list")]
    filters: Vec<Filter>,
    #[serde(default, deserialize_with = "json_list")]
    free_text: Vec<String>,
    #[serde(default, deserialize_with = "json_list")]
    measures: Vec<MeasureSpec>,
    #[serde(default, deserialize_with = "json_list")]
    group_by: Vec<String>,
    bucket_time: Option<i64>,
    origin_time: Option<i64>,
    order_by: Option<String>,
    /// `asc` or `desc`.
    order: Option<String>,
    limit: Option<usize>,
    /// Count what the rows that were kept stand for, by the share each was kept with, so that
    /// totals are those of every session (the default); `false` counts the rows that are stored.
    estimate: Option<bool>,
}

#[permission(any("rum.events.read", "sys.telemetry.read"))]
pub(super) async fn aggregate_post(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Json(request): Json<AggregateRequest>,
) -> Result<Json<Aggregate>> {
    aggregate(&state, &iam, request).await.map(Json)
}

#[permission(any("rum.events.read", "sys.telemetry.read"))]
pub(super) async fn aggregate_get(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(request): Query<AggregateRequest>,
) -> Result<Json<Aggregate>> {
    aggregate(&state, &iam, request).await.map(Json)
}

async fn aggregate(
    state: &AppState,
    iam: &IamContext,
    request: AggregateRequest,
) -> Result<Aggregate> {
    let selection = Selection::new(
        request.kind.as_deref(),
        request.from,
        request.to,
        request.filters,
        request.free_text,
    )?;

    let reader = Reader::new(state, iam);

    aggregate::run(
        &reader,
        &aggregate::Request {
            selection: &selection,
            measures: &request.measures,
            group_by: &request.group_by,
            bucket_micros: request.bucket_time,
            origin_micros: request.origin_time,
            order_by: request.order_by.as_deref(),
            descending: descending(request.order.as_deref())?,
            limit: request.limit,
            estimate: request.estimate.unwrap_or(true),
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use axum::http::Uri;

    use super::*;

    fn from_address(query: &str) -> std::result::Result<AggregateRequest, String> {
        let uri: Uri = format!("http://localhost/api/v1/rum/aggregate?{query}")
            .parse()
            .expect("uri");

        Query::<AggregateRequest>::try_from_uri(&uri)
            .map(|query| query.0)
            .map_err(|rejection| rejection.body_text())
    }

    /// Every byte but letters and digits escaped, as a browser may send them.
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
        let measures = r#"[{"fn":"p75","field":"view.loading_time","name":"slow","where":[{"field":"env","op":"=","value":"prod"}]},{"fn":"count"}]"#;
        let group_by = r#"["view.url_path_group"]"#;

        let address = from_address(&format!(
            "type=views&measures={}&groupBy={}&limit=5&orderBy=slow&order=asc",
            escaped(measures),
            escaped(group_by),
        ))
        .expect("address");

        let body: AggregateRequest = serde_json::from_value(serde_json::json!({
            "type": "views",
            "measures": [
                {
                    "fn": "p75",
                    "field": "view.loading_time",
                    "name": "slow",
                    "where": [{ "field": "env", "op": "=", "value": "prod" }],
                },
                { "fn": "count" },
            ],
            "groupBy": ["view.url_path_group"],
            "limit": 5,
            "orderBy": "slow",
            "order": "asc",
        }))
        .expect("body");

        assert_eq!(address.measures, body.measures);
        assert_eq!(address.group_by, body.group_by);
        assert_eq!(address.measures[0].filters[0].field, "env");
        assert_eq!(address.measures[1].function, "count");
        assert_eq!(address.limit, Some(5));
        assert_eq!(address.order.as_deref(), Some("asc"));
    }

    #[test]
    fn a_read_estimates_unless_it_asks_for_the_rows_that_are_stored() {
        let measures = escaped(r#"[{"fn":"count"}]"#);

        let left_out = from_address(&format!("type=views&measures={measures}")).expect("address");
        let stored = from_address(&format!("type=views&measures={measures}&estimate=false"))
            .expect("address");
        let said = from_address(&format!("type=views&measures={measures}&estimate=true"))
            .expect("address");

        assert_eq!(left_out.estimate, None);
        assert_eq!(stored.estimate, Some(false));
        assert_eq!(said.estimate, Some(true));
        assert!(from_address("estimate=sometimes").is_err());
    }

    #[test]
    fn an_empty_list_is_no_list_and_text_that_is_not_one_is_refused() {
        let empty = from_address("measures=&groupBy=").expect("address");

        assert!(empty.measures.is_empty());
        assert!(empty.group_by.is_empty());
        assert!(from_address("measures=count").is_err());
    }
}
