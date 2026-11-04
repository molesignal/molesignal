// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET|POST /rum/facets`: the commonest values of one field across the rows the explorer's query
//! matches, and how many rows have each.

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    address::json_list,
    reader::{Counted, Reader},
    selection::Selection,
};
use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    infra::rum::query::{Column, FACET_COUNT, FACET_VALUE, Filter, Scope, Unavailable, facet_sql},
    shared::{Error, Result},
};

const DEFAULT_VALUES: usize = 10;
const MAX_VALUES: usize = 100;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FacetRequest {
    #[serde(rename = "type")]
    kind: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    #[serde(default, deserialize_with = "json_list")]
    filters: Vec<Filter>,
    #[serde(default, deserialize_with = "json_list")]
    free_text: Vec<String>,
    field: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct FacetValue {
    value: String,
    count: u64,
}

#[derive(Debug, Serialize)]
pub(super) struct FacetResponse {
    field: String,
    /// The commonest first.
    values: Vec<FacetValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable: Option<Unavailable>,
}

#[permission(any("rum.events.read", "sys.telemetry.read"))]
pub(super) async fn facets_post(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Json(request): Json<FacetRequest>,
) -> Result<Json<FacetResponse>> {
    facets(&state, &iam, request).await.map(Json)
}

#[permission(any("rum.events.read", "sys.telemetry.read"))]
pub(super) async fn facets_get(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(request): Query<FacetRequest>,
) -> Result<Json<FacetResponse>> {
    facets(&state, &iam, request).await.map(Json)
}

async fn facets(
    state: &AppState,
    iam: &IamContext,
    request: FacetRequest,
) -> Result<FacetResponse> {
    let mut selection = Selection::new(
        request.kind.as_deref(),
        request.from,
        request.to,
        request.filters,
        request.free_text,
    )?;

    let name = request
        .field
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| Error::invalid("field is required"))?;

    // The kinds of session are read of every kind: else the other kinds could not be found.
    selection.every_kind = name == "session.type";

    let column = Column::resolve(selection.stream, name)?.ok_or_else(|| {
        Error::invalid(format!(
            "`{name}` is not a field of {}",
            selection.stream.name()
        ))
    })?;

    let conditions = selection.conditions()?;

    let limit = request.limit.unwrap_or(DEFAULT_VALUES).clamp(1, MAX_VALUES);

    let statement = facet_sql(
        &Scope {
            stream: selection.stream,
            from: selection.span.from,
            to: selection.span.to,
            conditions: &conditions,
        },
        &column,
        limit,
    )?;

    let answer = |found, unavailable| FacetResponse {
        field: column.name().to_string(),
        values: found,
        unavailable,
    };

    match Reader::new(state, iam)
        .count(selection.stream, selection.span, statement)
        .await?
    {
        Counted::Rows(Some(result)) => Ok(answer(values(&result.columns, &result.rows)?, None)),
        Counted::Rows(None) => Ok(answer(Vec::new(), None)),
        Counted::Unavailable(reason) => Ok(answer(Vec::new(), Some(reason))),
    }
}

/// The values out of the rows of [`facet_sql`]. A masked value comes back as the text it was
/// masked to, and is counted as such.
fn values(columns: &[String], rows: &[Vec<Value>]) -> Result<Vec<FacetValue>> {
    // A statement that found no rows says nothing of its columns: there are no values.
    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let position = |name: &str| columns.iter().position(|column| column == name);

    let (Some(value), Some(count)) = (position(FACET_VALUE), position(FACET_COUNT)) else {
        return Err(Error::internal("facet statement returned no values"));
    };

    Ok(rows
        .iter()
        .filter_map(|row| {
            let count = row.get(count).and_then(Value::as_u64)?;

            let value = match row.get(value)? {
                Value::String(text) => text.clone(),
                Value::Null => return None,
                other => other.to_string(),
            };

            Some(FacetValue { value, count })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn columns() -> Vec<String> {
        [FACET_VALUE, FACET_COUNT].map(String::from).to_vec()
    }

    #[test]
    fn values_are_read_in_the_order_they_come() {
        let found = values(
            &columns(),
            &[
                vec![json!("chrome"), json!(40)],
                vec![json!("safari"), json!(7)],
            ],
        )
        .unwrap();

        assert_eq!(
            found,
            vec![
                FacetValue {
                    value: "chrome".into(),
                    count: 40
                },
                FacetValue {
                    value: "safari".into(),
                    count: 7
                },
            ]
        );
    }

    #[test]
    fn a_value_that_is_not_text_is_read_as_text_and_a_null_is_left_out() {
        let found = values(
            &columns(),
            &[
                vec![json!(404), json!(3)],
                vec![Value::Null, json!(2)],
                vec![json!("***"), json!(1)],
            ],
        )
        .unwrap();

        assert_eq!(
            found
                .iter()
                .map(|found| found.value.as_str())
                .collect::<Vec<_>>(),
            vec!["404", "***"]
        );
    }

    #[test]
    fn rows_without_the_columns_are_an_error() {
        assert!(values(&["other".to_string()], &[vec![json!(1)]]).is_err());
    }

    #[test]
    fn a_statement_that_found_no_rows_says_nothing_of_its_columns() {
        // DataFusion gives no batches for it, so the result has no columns at all.
        assert!(values(&[], &[]).unwrap().is_empty());
    }
}
