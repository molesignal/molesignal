// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The log reads of the explorer said in the query string. `GET /web/logs`,
//! `/web/logs/histogram` and `/web/logs/field-stats` take what the bodies of
//! their `POST`s take and answer what they answer, so a read is an address like
//! the page that made it. The filters and the words to find are JSON lists in
//! their parameters, as the trace list takes its filters, and the condition
//! tree is the text of a JSON object; a query too long for an address is still
//! sent as a `POST` body.

use std::{fmt, marker::PhantomData};

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::{
    Deserialize, Deserializer,
    de::{self, DeserializeOwned, SeqAccess, Visitor},
};
use serde_json::{Map, Value};

use super::{LogListRequest, field_stats::LogFieldStatsRequest, histogram::LogHistogramRequest};
use crate::{
    api::{
        AppState,
        http::{
            pagination::cursor::CursorPage,
            routes::web::{field_stats::FieldStatsResponse, histogram::HistogramResponse},
        },
    },
    app::iam::IamContext,
    domain::iam::permission,
    shared::Result,
};

/// A list that a body holds as a JSON array and an address as the text of one:
/// `filters=[{"field":"level","op":"=","value":"error"}]`. Empty text is no list.
pub(super) fn json_list<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    struct ListVisitor<T>(PhantomData<T>);

    impl<'de, T: DeserializeOwned> Visitor<'de> for ListVisitor<T> {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a list, or a JSON list as text")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Vec<T>, A::Error> {
            let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0));

            while let Some(item) = seq.next_element()? {
                items.push(item);
            }

            Ok(items)
        }

        fn visit_str<E: de::Error>(self, text: &str) -> std::result::Result<Vec<T>, E> {
            if text.trim().is_empty() {
                return Ok(Vec::new());
            }

            serde_json::from_str(text).map_err(E::custom)
        }
    }

    deserializer.deserialize_any(ListVisitor(PhantomData))
}

/// A value that a body holds as JSON and an address as the text of it:
/// `where={"or":[...]}`. Empty text is no value.
pub(super) fn json_value<'de, D>(deserializer: D) -> std::result::Result<Option<Value>, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::<Value>::deserialize(deserializer)? {
        Some(Value::String(text)) if text.trim().is_empty() => Ok(None),
        Some(Value::String(text)) => serde_json::from_str(&text)
            .map(Some)
            .map_err(de::Error::custom),
        Some(Value::Null) | None => Ok(None),
        value => Ok(value),
    }
}

#[permission(any("logs.read", "sys.telemetry.read"))]
pub(super) async fn list(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(request): Query<LogListRequest>,
) -> Result<Json<CursorPage<Map<String, Value>>>> {
    super::list(State(state), Extension(iam), Json(request)).await
}

#[permission(any("logs.read", "sys.telemetry.read"))]
pub(super) async fn histogram(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(request): Query<LogHistogramRequest>,
) -> Result<Json<HistogramResponse>> {
    super::histogram::histogram(State(state), Extension(iam), Json(request)).await
}

#[permission(any("logs.read", "sys.telemetry.read"))]
pub(super) async fn field_stats(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(request): Query<LogFieldStatsRequest>,
) -> Result<Json<FieldStatsResponse>> {
    super::field_stats::field_stats(State(state), Extension(iam), Json(request)).await
}

#[cfg(test)]
mod tests {
    use axum::http::Uri;

    use super::*;
    use crate::api::http::routes::web::logs::LogFilter;

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

    fn from_address(query: &str) -> std::result::Result<LogListRequest, String> {
        let uri: Uri = format!("http://localhost/api/v1/web/logs?{query}")
            .parse()
            .expect("uri");

        Query::<LogListRequest>::try_from_uri(&uri)
            .map(|query| query.0)
            .map_err(|rejection| rejection.body_text())
    }

    #[test]
    fn an_address_says_what_a_body_says() {
        let filters = r#"[{"field":"level","op":"=","value":"error","quoted":true}]"#;
        let address = from_address(&format!(
            "stream=app&from=10&to=20&limit=100&filters={}&freeText={}",
            escaped(filters),
            escaped(r#"["time out"]"#),
        ))
        .expect("address");
        let body: LogListRequest = serde_json::from_value(serde_json::json!({
            "stream": "app",
            "from": 10,
            "to": 20,
            "limit": 100,
            "filters": [{ "field": "level", "op": "=", "value": "error", "quoted": true }],
            "freeText": ["time out"],
        }))
        .expect("body");

        assert_eq!(address.filters, body.filters);
        assert_eq!(
            address.filters,
            vec![LogFilter {
                field: "level".into(),
                op: "=".into(),
                value: "error".into(),
                quoted: true,
            }]
        );
        assert_eq!(address.free_text, body.free_text);
        assert_eq!(
            (address.stream, address.from, address.to, address.limit),
            (body.stream, body.from, body.to, body.limit)
        );
    }

    #[test]
    fn an_address_says_the_condition_tree_a_body_says() {
        let tree = serde_json::json!({"or": [
            {"field": "level", "op": "=", "value": "error", "quoted": true},
            {"text": "time out"}
        ]});
        let address = from_address(&format!("stream=app&where={}", escaped(&tree.to_string())))
            .expect("address");
        let body: LogListRequest =
            serde_json::from_value(serde_json::json!({"stream": "app", "where": tree}))
                .expect("body");

        assert_eq!(address.condition, Some(tree));
        assert_eq!(address.condition, body.condition);

        let empty = from_address("stream=app&where=").expect("address");
        assert_eq!(empty.condition, None);

        let refused = from_address("stream=app&where=level").expect_err("not JSON");
        assert!(refused.contains("where"), "{refused}");
    }

    #[test]
    fn an_empty_list_is_no_list_and_text_that_is_not_one_is_refused() {
        let empty = from_address("stream=app&filters=&freeText=").expect("address");

        assert!(empty.filters.is_empty());
        assert!(empty.free_text.is_empty());

        let refused = from_address("stream=app&filters=level").expect_err("not a list");

        assert!(refused.contains("filters"), "{refused}");
    }
}
