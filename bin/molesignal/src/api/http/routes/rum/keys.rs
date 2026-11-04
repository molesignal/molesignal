// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET|POST /rum/keys`: the keys found in a JSON column across the rows a query matches, and how
//! many sessions had each: the feature flags that were evaluated, the custom timings that were
//! marked. A key is then named like a column, as `feature_flags.<key>`.

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
    domain::{iam::permission, rum::event::field},
    infra::rum::query::{
        Filter, KEY_COLUMN, KEY_COUNT, MEMBER_COLUMNS, Scope, Unavailable, keys_sql,
    },
    shared::{Error, Result},
};

const DEFAULT_KEYS: usize = 100;
const MAX_KEYS: usize = 500;

#[derive(Debug, Default, Deserialize)]
pub(super) struct KeysRequest {
    #[serde(rename = "type")]
    kind: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    #[serde(default, deserialize_with = "json_list")]
    filters: Vec<Filter>,
    /// `feature_flags`, `context`, `view.custom_timings` or `usr.extra`.
    column: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct KeyView {
    key: String,
    sessions: u64,
}

#[derive(Debug, Serialize)]
pub(super) struct KeysResponse {
    column: String,
    /// The one most sessions had first.
    keys: Vec<KeyView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable: Option<Unavailable>,
}

#[permission(any("rum.events.read", "sys.telemetry.read"))]
pub(super) async fn keys_post(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Json(request): Json<KeysRequest>,
) -> Result<Json<KeysResponse>> {
    keys(&state, &iam, request).await.map(Json)
}

#[permission(any("rum.events.read", "sys.telemetry.read"))]
pub(super) async fn keys_get(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(request): Query<KeysRequest>,
) -> Result<Json<KeysResponse>> {
    keys(&state, &iam, request).await.map(Json)
}

async fn keys(state: &AppState, iam: &IamContext, request: KeysRequest) -> Result<KeysResponse> {
    let selection = Selection::new(
        request.kind.as_deref().or(Some("views")),
        request.from,
        request.to,
        request.filters,
        Vec::new(),
    )?;

    let column = request
        .column
        .as_deref()
        .map(str::trim)
        .filter(|column| MEMBER_COLUMNS.contains(column))
        .ok_or_else(|| {
            Error::invalid(format!(
                "column must be one of {}",
                MEMBER_COLUMNS.join(", ")
            ))
        })?;

    if field(selection.stream, column).is_none() {
        return Err(Error::invalid(format!(
            "`{column}` is not a field of {}",
            selection.stream.name()
        )));
    }

    let conditions = selection.conditions()?;
    let limit = request.limit.unwrap_or(DEFAULT_KEYS).clamp(1, MAX_KEYS);

    let statement = keys_sql(
        &Scope {
            stream: selection.stream,
            from: selection.span.from,
            to: selection.span.to,
            conditions: &conditions,
        },
        column,
        limit,
    );

    let answer = |keys, unavailable| KeysResponse {
        column: column.to_string(),
        keys,
        unavailable,
    };

    match Reader::new(state, iam)
        .count(selection.stream, selection.span, statement)
        .await?
    {
        Counted::Rows(Some(result)) => Ok(answer(found(&result.columns, &result.rows), None)),
        Counted::Rows(None) => Ok(answer(Vec::new(), None)),
        Counted::Unavailable(reason) => Ok(answer(Vec::new(), Some(reason))),
    }
}

fn found(columns: &[String], rows: &[Vec<Value>]) -> Vec<KeyView> {
    let at = |name: &str| columns.iter().position(|column| column == name);

    let (Some(key), Some(sessions)) = (at(KEY_COLUMN), at(KEY_COUNT)) else {
        return Vec::new();
    };

    rows.iter()
        .filter_map(|row| {
            Some(KeyView {
                key: row.get(key)?.as_str()?.to_string(),
                sessions: row.get(sessions)?.as_u64().unwrap_or_default(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_keys_come_with_how_many_sessions_had_them() {
        let columns = vec![KEY_COLUMN.to_string(), KEY_COUNT.to_string()];

        assert_eq!(
            found(
                &columns,
                &[
                    vec![json!("checkout_v2"), json!(12)],
                    vec![Value::Null, json!(1)]
                ]
            ),
            [KeyView {
                key: "checkout_v2".into(),
                sessions: 12
            }]
        );
        assert!(found(&[], &[]).is_empty());
    }
}
