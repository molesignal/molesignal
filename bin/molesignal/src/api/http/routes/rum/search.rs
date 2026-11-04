// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET|POST /rum/search`: a page of the rows of the explorer, newest first, with signed keyset
//! cursors in both directions.

mod cursor;

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::Deserialize;
use serde_json::{Map, Value};

use super::{
    address::json_list,
    reader::{Reader, Row, rows},
    selection::Selection,
};
use crate::{
    api::{
        AppState,
        http::pagination::cursor::{CursorDirection, CursorPage, trim_cursor_page},
    },
    app::iam::IamContext,
    domain::iam::permission,
    infra::rum::query::{Filter, ListRequest, Scope, Seek, list_sql, requested_columns},
    shared::{Error, Result},
};

const DEFAULT_PAGE_SIZE: usize = 100;
/// The most rows one answer holds. The explorer offers pages up to this size
/// (`EXPLORER_PAGE_SIZE_OPTIONS` in the web app), so the two change together.
pub(super) const MAX_PAGE_SIZE: usize = 5_000;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SearchRequest {
    /// What is listed: `sessions`, `views`, `actions`, `errors`, `resources`, `long_tasks` or
    /// `vitals`.
    #[serde(rename = "type")]
    kind: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    #[serde(default, deserialize_with = "json_list")]
    filters: Vec<Filter>,
    #[serde(default, deserialize_with = "json_list")]
    free_text: Vec<String>,
    /// The columns to answer with; none asked, the defaults of the type.
    #[serde(default, deserialize_with = "json_list")]
    columns: Vec<String>,
    limit: Option<usize>,
    cursor: Option<String>,
}

/// A page of the list: what it selects, the columns it answers with and where it begins.
pub(super) struct Context {
    selection: Selection,
    columns: Vec<String>,
    page_size: usize,
    boundary: Option<Seek>,
}

#[permission(any("rum.events.read", "sys.telemetry.read"))]
pub(super) async fn search_post(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<CursorPage<Map<String, Value>>>> {
    search(&state, &iam, request).await.map(Json)
}

#[permission(any("rum.events.read", "sys.telemetry.read"))]
pub(super) async fn search_get(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(request): Query<SearchRequest>,
) -> Result<Json<CursorPage<Map<String, Value>>>> {
    search(&state, &iam, request).await.map(Json)
}

async fn search(
    state: &AppState,
    iam: &IamContext,
    request: SearchRequest,
) -> Result<CursorPage<Map<String, Value>>> {
    let context = resolve(state, iam, request)?;
    let stream = context.selection.stream;
    let span = context.selection.span;

    let conditions = context.selection.conditions()?;
    let columns = requested_columns(stream, &context.columns)?;
    let fetch = context.page_size.saturating_add(1);

    let statement = list_sql(&ListRequest {
        scope: Scope {
            stream,
            from: span.from,
            to: span.to,
            conditions: &conditions,
        },
        columns: &columns,
        limit: fetch,
        seek: context.boundary.clone(),
        oldest_first: false,
    });

    let Some(result) = Reader::new(state, iam)
        .run(stream, span, statement, Some(fetch))
        .await?
    else {
        return Ok(CursorPage::empty());
    };

    let direction = context.boundary.as_ref().map(|seek| seek.direction);
    let page = trim_cursor_page(rows(stream, result), context.page_size, direction);

    let previous_cursor = page
        .has_previous
        .then(|| page.items.first())
        .flatten()
        .map(|row| cursor_at(state, iam, &context, CursorDirection::Before, row))
        .transpose()?;

    let next_cursor = page
        .has_next
        .then(|| page.items.last())
        .flatten()
        .map(|row| cursor_at(state, iam, &context, CursorDirection::After, row))
        .transpose()?;

    Ok(CursorPage {
        items: page.items.into_iter().map(Row::into_item).collect(),
        has_more: next_cursor.is_some(),
        next_cursor,
        previous_cursor,
    })
}

fn cursor_at(
    state: &AppState,
    iam: &IamContext,
    context: &Context,
    direction: CursorDirection,
    row: &Row,
) -> Result<String> {
    cursor::encode(
        state.iam.service.as_ref(),
        &iam.org_id,
        context,
        direction,
        (row.timestamp_micros, &row.tie),
    )
}

fn resolve(state: &AppState, iam: &IamContext, request: SearchRequest) -> Result<Context> {
    let Some(token) = request.cursor.as_deref() else {
        let selection = Selection::new(
            request.kind.as_deref(),
            request.from,
            request.to,
            request.filters,
            request.free_text,
        )?;

        return Ok(Context {
            selection,
            columns: request.columns,
            page_size: request
                .limit
                .unwrap_or(DEFAULT_PAGE_SIZE)
                .clamp(1, MAX_PAGE_SIZE),
            boundary: None,
        });
    };

    let payload = cursor::decode(state.iam.service.as_ref(), &iam.org_id, token)?;

    if disagrees(&request, &payload) {
        return Err(Error::invalid("RUM cursor does not match the active query"));
    }

    let selection = Selection::new(
        Some(&payload.collection),
        Some(payload.from),
        Some(payload.to),
        payload.filters,
        payload.free_text,
    )?;

    Ok(Context {
        selection,
        columns: payload.columns,
        page_size: payload.page_size,
        boundary: Some(Seek {
            direction: payload.direction,
            ts: payload.timestamp,
            tie: payload.tie,
        }),
    })
}

/// Whether the request that carries a cursor asks for something else than the cursor was made
/// for. What it leaves out the cursor decides.
fn disagrees(request: &SearchRequest, payload: &cursor::Payload) -> bool {
    request
        .kind
        .as_deref()
        .is_some_and(|kind| kind != payload.collection)
        || request.from.is_some_and(|from| from != payload.from)
        || request.to.is_some_and(|to| to != payload.to)
        || request
            .limit
            .is_some_and(|limit| limit.clamp(1, MAX_PAGE_SIZE) != payload.page_size)
        || (!request.filters.is_empty() && request.filters != payload.filters)
        || (!request.free_text.is_empty() && request.free_text != payload.free_text)
        || (!request.columns.is_empty() && request.columns != payload.columns)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn payload() -> cursor::Payload {
        serde_json::from_value(json!({
            "version": 1,
            "collection": "errors",
            "from": 10,
            "to": 20,
            "pageSize": 100,
            "filters": [{ "field": "env", "op": "=", "value": "prod" }],
            "freeText": ["timeout"],
            "columns": [],
            "direction": "after",
            "timestamp": 15,
            "tie": "b:2",
        }))
        .expect("payload")
    }

    fn request(value: Value) -> SearchRequest {
        serde_json::from_value(value).expect("request")
    }

    #[test]
    fn a_request_that_leaves_things_out_follows_its_cursor() {
        assert!(!disagrees(&request(json!({ "cursor": "x" })), &payload()));
        assert!(!disagrees(
            &request(json!({
                "type": "errors",
                "from": 10,
                "to": 20,
                "limit": 100,
                "filters": [{ "field": "env", "op": "=", "value": "prod" }],
                "freeText": ["timeout"],
            })),
            &payload()
        ));
    }

    #[test]
    fn a_request_that_asks_for_something_else_than_its_cursor_is_refused() {
        for changed in [
            json!({ "type": "views" }),
            json!({ "from": 11 }),
            json!({ "to": 21 }),
            json!({ "limit": 50 }),
            json!({ "filters": [{ "field": "env", "op": "=", "value": "dev" }] }),
            json!({ "freeText": ["other"] }),
            json!({ "columns": ["env"] }),
        ] {
            assert!(
                disagrees(&request(changed.clone()), &payload()),
                "{changed}"
            );
        }
    }

    #[test]
    fn an_address_says_what_a_body_says() {
        let from_address: SearchRequest = serde_json::from_value(json!({
            "type": "views",
            "from": 1,
            "to": 2,
            "filters": r#"[{"field":"env","op":"=","value":"prod"}]"#,
            "columns": r#"["env"]"#,
            "freeText": "",
        }))
        .expect("address");

        assert_eq!(from_address.filters.len(), 1);
        assert_eq!(from_address.columns, vec!["env".to_string()]);
        assert!(from_address.free_text.is_empty());
    }
}
