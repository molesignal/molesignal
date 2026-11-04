// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET /rum/schema/{type}`: the columns of one kind of row, which the explorer builds its field
//! list, its column picker and its filters from.

use axum::{Extension, Json, extract::Path};
use serde::Serialize;

use super::selection::stream_of;
use crate::{
    app::iam::IamContext,
    domain::{
        iam::permission,
        rum::event::{RumStream, fields},
        stream::{FieldType, StreamIndexType},
    },
    infra::rum::query::{default_columns, is_facetable, is_internal},
    shared::Result,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SchemaField {
    name: &'static str,
    data_type: FieldType,
    /// `ns`, `ms`, `bytes`, `count`, `score`, `percent`, `per_second`, `px` or `none`. A duration
    /// is stored in the nanoseconds the SDK sent; the page formats it by this.
    unit: &'static str,
    /// Its values can be counted; a JSON document has none to count.
    facetable: bool,
    indexed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SchemaResponse {
    #[serde(rename = "type")]
    kind: &'static str,
    stream: &'static str,
    /// The column that names one row.
    id_field: &'static str,
    /// Rows are reported again as the entity changes; the page reads the latest.
    versioned: bool,
    default_columns: Vec<&'static str>,
    fields: Vec<SchemaField>,
}

#[permission(any("rum.events.read", "sys.telemetry.read"))]
pub(super) async fn schema(
    Extension(iam): Extension<IamContext>,
    Path(kind): Path<String>,
) -> Result<Json<SchemaResponse>> {
    Ok(Json(describe(stream_of(&kind)?)))
}

fn describe(stream: RumStream) -> SchemaResponse {
    SchemaResponse {
        kind: stream.collection(),
        stream: stream.name(),
        id_field: stream.id_field(),
        versioned: stream.versioned().is_some(),
        default_columns: default_columns(stream).to_vec(),
        fields: fields(stream)
            .iter()
            .filter(|column| !is_internal(column.name))
            .map(|column| SchemaField {
                name: column.name,
                data_type: column.data_type,
                unit: column.unit.as_str(),
                facetable: is_facetable(stream, column.name),
                indexed: column.index != StreamIndexType::None,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_schema_lists_the_columns_with_their_units() {
        let schema = describe(RumStream::Views);

        assert_eq!(schema.kind, "views");
        assert_eq!(schema.stream, "rum_views");
        assert_eq!(schema.id_field, "view.id");
        assert!(schema.versioned);

        let loading = schema
            .fields
            .iter()
            .find(|column| column.name == "view.loading_time")
            .expect("loading time");

        assert_eq!(loading.unit, "ns");
        assert_eq!(loading.data_type, FieldType::Int64);
        assert!(loading.facetable);

        let context = schema
            .fields
            .iter()
            .find(|column| column.name == "context")
            .expect("context");

        assert!(!context.facetable);

        let session = schema
            .fields
            .iter()
            .find(|column| column.name == "session.id")
            .expect("session id");

        assert!(session.indexed);
    }

    #[test]
    fn the_default_columns_are_columns_of_the_schema() {
        for stream in RumStream::ALL {
            let schema = describe(stream);

            for name in &schema.default_columns {
                assert!(
                    schema.fields.iter().any(|column| column.name == *name),
                    "{name} in {stream:?}"
                );
            }
        }
    }

    #[test]
    fn a_schema_serializes_the_type_under_its_own_name() {
        let value = serde_json::to_value(describe(RumStream::LongTasks)).unwrap();

        assert_eq!(value["type"], "long_tasks");
        assert!(value["fields"][0]["dataType"].is_string());
    }
}
