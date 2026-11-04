// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The reads of the RUM pages said in the query string. `GET /rum/search`, `/rum/facets` and
//! `/rum/histogram` take what the bodies of their `POST`s take and answer what they answer, so a
//! read is an address like the page that made it. The filters and the words to find are JSON
//! lists in their parameters; a query too long for an address is still sent as a `POST` body.

use std::{fmt, marker::PhantomData};

use serde::{
    Deserializer,
    de::{self, DeserializeOwned, SeqAccess, Visitor},
};

/// A list that a body holds as a JSON array and an address as the text of one:
/// `filters=[{"field":"env","op":"=","value":"prod"}]`. Empty text is no list.
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

#[cfg(test)]
mod tests {
    use axum::{extract::Query, http::Uri};
    use serde::Deserialize;

    use super::*;
    use crate::infra::rum::query::Filter;

    #[derive(Debug, Deserialize)]
    struct Address {
        #[serde(default, deserialize_with = "json_list")]
        filters: Vec<Filter>,
        #[serde(default, deserialize_with = "json_list")]
        free_text: Vec<String>,
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

    fn from_address(query: &str) -> std::result::Result<Address, String> {
        let uri: Uri = format!("http://localhost/api/v1/rum/search?{query}")
            .parse()
            .expect("uri");

        Query::<Address>::try_from_uri(&uri)
            .map(|query| query.0)
            .map_err(|rejection| rejection.body_text())
    }

    #[test]
    fn an_address_says_what_a_body_says() {
        let filters = r#"[{"field":"env","op":"=","value":"prod"}]"#;
        let address = from_address(&format!(
            "filters={}&freeText={}",
            escaped(filters),
            escaped(r#"["time out"]"#),
        ))
        .expect("address");
        let body: Address = serde_json::from_value(serde_json::json!({
            "filters": [{ "field": "env", "op": "=", "value": "prod" }],
            "freeText": ["time out"],
        }))
        .expect("body");

        assert_eq!(address.filters, body.filters);
        assert_eq!(address.free_text, body.free_text);
        assert_eq!(address.filters[0].field, "env");
    }

    #[test]
    fn an_empty_list_is_no_list_and_text_that_is_not_one_is_refused() {
        let empty = from_address("filters=&freeText=").expect("address");

        assert!(empty.filters.is_empty());
        assert!(empty.free_text.is_empty());

        let refused = from_address("filters=env").expect_err("not a list");

        assert!(refused.contains("filters"), "{refused}");
    }
}
