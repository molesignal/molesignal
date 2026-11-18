// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use arrow::{
    array::{Array, AsArray},
    datatypes::{Float64Type, Int64Type, TimestampMicrosecondType},
};
use domain::{
    intake::RawEvent,
    stream::{FieldDef, StreamDefinition, StreamIndexType, StreamType},
};
use serde_json::{Value as Json, json};

use super::*;
use crate::{
    app::intake::{check_event_types, usage::ServiceTally},
    infra::intake::row_columns::rows_to_columns,
};

const NOW: TimestampMicros = TimestampMicros(1_700_000_000_000_000);

fn field(name: &str, data_type: FieldType, nullable: bool) -> FieldDef {
    FieldDef {
        name: name.into(),
        data_type,
        nullable,
        index_type: StreamIndexType::None,
        encrypted: false,
    }
}

fn run(body: &Json, schema: &Schema) -> Result<Decoded> {
    run_with(body, schema, Options::default())
}

fn run_with(body: &Json, schema: &Schema, options: Options<'_>) -> Result<Decoded> {
    let mut bytes = serde_json::to_vec(body).unwrap();
    decode(&mut bytes, schema, &Id::from_string("b1"), NOW, options)
}

fn column<'a>(decoded: &'a Decoded, name: &str) -> &'a ArrayRef {
    decoded
        .columns
        .column_by_name(name)
        .unwrap_or_else(|| panic!("no column {name}"))
}

/// The row path, event by event: parse, take `_timestamp` out, count the bytes, set `_event_id`,
/// check the types, then build the buffer columns.
fn row_path(body: &Json, schema: &Schema) -> (RecordBatch, Vec<usize>, u64) {
    let items = match body {
        Json::Array(items) => items.clone(),
        other => vec![other.clone()],
    };

    let mut kept = Vec::new();
    let mut rejected = Vec::new();
    let mut bytes = 0;

    for (index, item) in items.into_iter().enumerate() {
        let Json::Object(mut fields) = item else {
            unreachable!()
        };

        let timestamp = fields
            .remove("_timestamp")
            .and_then(|value| value.as_i64())
            .map_or(NOW, TimestampMicros);

        let mut event = RawEvent { timestamp, fields };
        let size = domain::intake::usage::event_bytes(&event).unwrap();

        event
            .fields
            .insert(EVENT_ID_FIELD.into(), Json::String(format!("b1:{index}")));

        match check_event_types(schema, &event) {
            Ok(()) => {
                bytes += size;
                kept.push(event);
            }
            Err(_) => rejected.push(index),
        }
    }

    let extended = crate::app::intake::infer_schema_extension(schema, &kept, StreamType::LOGS)
        .unwrap_or_else(|| schema.clone());

    let stream = StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string("org"),
        name: "app".into(),
        stream_type: StreamType::LOGS,
        schema: extended,
        retention: None,
        created_at: NOW,
        updated_at: NOW,
    };

    (
        rows_to_columns(&stream, &kept, None).unwrap(),
        rejected,
        bytes,
    )
}

/// The columns, the rejected rows and the usage bytes match the row path's, column by column.
#[test]
fn a_body_decodes_as_the_row_path_writes_it() {
    let schema = Schema {
        fields: vec![
            field("level", FieldType::Utf8, false),
            field("latency", FieldType::Int64, true),
            field("ratio", FieldType::Float64, true),
            field("attrs", FieldType::Json, true),
        ],
    };

    let body = json!([
        {"_timestamp": 5, "level": "info", "latency": 12, "ratio": 1, "attrs": {"a": [1, "x"]},
         "message": "中文 \"quoted\"\n", "service": "checkout"},
        {"level": "warn", "latency": "slow"},
        {"latency": 3},
        {"_timestamp": 7.5, "level": "error", "ratio": 2.5, "attrs": "plain", "extra": null,
         "message": "second", "count": 3},
        {"_timestamp": 9, "level": "info", "message": "dup", "message": "dup2", "_event_id": "x"}
    ]);

    let decoded = run(&body, &schema).unwrap();
    let (expected, rejected, bytes) = row_path(&body, &schema);

    let indexes: Vec<_> = decoded.errors.iter().map(|error| error.index).collect();
    assert_eq!(indexes, rejected);
    assert_eq!(decoded.collected_bytes, bytes);
    assert_eq!(decoded.columns.num_rows(), expected.num_rows());

    for field in expected.schema().fields() {
        let ours = column(&decoded, field.name());
        let theirs = expected.column_by_name(field.name()).unwrap();

        assert_eq!(ours.data_type(), theirs.data_type(), "{}", field.name());
        assert_eq!(ours.as_ref(), theirs.as_ref(), "{}", field.name());
    }

    assert_eq!(
        decoded.columns.num_columns(),
        expected.num_columns(),
        "{:?}",
        decoded.columns.schema()
    );
}

#[test]
fn rejections_name_the_first_failing_field_in_schema_order() {
    let schema = Schema {
        fields: vec![
            field("level", FieldType::Utf8, false),
            field("latency", FieldType::Int64, true),
        ],
    };

    let body = json!([
        {"latency": "slow"},
        {"level": "info", "latency": 1.5},
        {"level": 3}
    ]);

    let decoded = run(&body, &schema).unwrap();
    let reasons: Vec<_> = decoded
        .errors
        .iter()
        .map(|error| (error.index, error.reason.as_str()))
        .collect();

    assert_eq!(
        reasons,
        [
            (0, "missing non-null field `level`"),
            (
                1,
                "type mismatch on field `latency`: expected Int64, got Number"
            ),
            (
                2,
                "type mismatch on field `level`: expected Utf8, got Number"
            ),
        ]
    );
    assert_eq!(decoded.columns.num_rows(), 0);
    assert!(decoded.new_fields.is_empty());
}

#[test]
fn new_fields_take_the_type_of_their_first_kept_value() {
    let schema = Schema {
        fields: vec![field("level", FieldType::Utf8, false)],
    };

    let body = json!([
        {"latency": "from a rejected row"},
        {"level": "a", "latency": 5, "nothing": null},
        {"level": "b", "latency": "now a string"},
        {"level": "c", "latency": 6}
    ]);

    let decoded = run(&body, &schema).unwrap();

    assert_eq!(
        decoded.new_fields,
        BTreeMap::from([
            ("_event_id".to_owned(), FieldType::Utf8),
            ("latency".to_owned(), FieldType::Int64),
            ("nothing".to_owned(), FieldType::Json),
        ])
    );

    let indexes: Vec<_> = decoded.errors.iter().map(|error| error.index).collect();
    assert_eq!(indexes, [0, 2]);

    let latency = column(&decoded, "latency").as_primitive::<Int64Type>();
    assert_eq!(latency.values().as_ref(), [5, 6]);
}

#[test]
fn timestamps_and_event_ids_come_from_the_row_and_the_batch() {
    let schema = Schema {
        fields: vec![field(EVENT_ID_FIELD, FieldType::Utf8, true)],
    };

    let body = json!([
        {"_timestamp": 42, "_event_id": "spoofed"},
        {"_timestamp": "late"},
        {}
    ]);

    let decoded = run(&body, &schema).unwrap();
    let times = column(&decoded, "_timestamp").as_primitive::<TimestampMicrosecondType>();
    let ids = column(&decoded, EVENT_ID_FIELD).as_string::<i32>();

    assert_eq!(times.values().as_ref(), [42, NOW.0, NOW.0]);
    assert_eq!(
        ids.iter().flatten().collect::<Vec<_>>(),
        ["b1:0", "b1:1", "b1:2"]
    );
    assert!(decoded.new_fields.is_empty());
}

#[test]
fn every_row_is_counted_for_its_service() {
    let schema = Schema {
        fields: vec![field("level", FieldType::Utf8, false)],
    };

    let body = json!([
        {"level": "info", "service": "checkout", "service.name": "api"},
        {"service": "checkout"},
        {"level": "info", "service_name": 7}
    ]);

    let decoded = run(&body, &schema).unwrap();
    let mut expected = ServiceTally::default();
    expected.add("api", 1);
    expected.add("checkout", 1);
    expected.add(UNNAMED_SERVICE, 1);

    assert_eq!(decoded.services, expected);
    assert_eq!(decoded.errors.len(), 1);
}

#[test]
fn a_single_object_is_one_row() {
    let decoded = run(&json!({"message": "hi"}), &Schema { fields: vec![] }).unwrap();

    assert_eq!(decoded.columns.num_rows(), 1);
    assert_eq!(
        column(&decoded, "message").as_string::<i32>().value(0),
        "hi"
    );
}

#[test]
fn bodies_that_are_not_rows_are_refused() {
    let schema = Schema { fields: vec![] };

    assert!(matches!(
        run(&json!([{"a": 1}, 2]), &schema),
        Err(Error::InvalidArgument(message)) if message == "event #1 must be a JSON object"
    ));
    assert!(matches!(
        run(&json!("text"), &schema),
        Err(Error::InvalidArgument(_))
    ));
    assert!(matches!(
        decode(
            &mut b"[{".to_vec(),
            &schema,
            &Id::from_string("b1"),
            NOW,
            Options::default()
        ),
        Err(Error::InvalidArgument(_))
    ));
}

#[test]
fn floats_and_wide_integers_land_in_float_columns() {
    let schema = Schema {
        fields: vec![field("ratio", FieldType::Float64, true)],
    };

    let decoded = run(
        &json!([{"ratio": 1}, {"ratio": u64::MAX}, {"ratio": 0.25}]),
        &schema,
    )
    .unwrap();
    let ratio = column(&decoded, "ratio").as_primitive::<Float64Type>();

    assert_eq!(ratio.values().as_ref(), [1.0, u64::MAX as f64, 0.25]);
    assert!(!ratio.is_null(1));
}

#[test]
fn strings_are_masked_after_they_are_measured() {
    let masker = Masker::compile([("\\d{4}-\\d{4}".to_owned(), "****".to_owned())]);
    let schema = Schema {
        fields: vec![field("attrs", FieldType::Json, true)],
    };

    let body = json!([{"card": "1234-5678", "attrs": {"card": "1234-5678", "n": 1}}]);
    let plain = run(&body, &schema).unwrap();
    let masked = run_with(
        &body,
        &schema,
        Options {
            masker: Some(&masker),
            watched: None,
        },
    )
    .unwrap();

    assert_eq!(column(&masked, "card").as_string::<i32>().value(0), "****");
    assert_eq!(
        column(&masked, "attrs").as_string::<i32>().value(0),
        r#"{"card":"****","n":1}"#
    );
    assert_eq!(masked.collected_bytes, plain.collected_bytes);
}

/// The text left for the sink to encrypt is the row path's plain text: numbers as sent.
#[test]
fn encrypted_fields_keep_their_values_as_text() {
    let mut code = field("code", FieldType::Float64, true);
    code.encrypted = true;

    let mut tags = field("tags", FieldType::Json, true);
    tags.encrypted = true;

    let schema = Schema {
        fields: vec![code, tags],
    };

    let decoded = run(
        &json!([{"code": 3, "tags": ["a", 1]}, {"code": 2.5, "tags": "plain"}, {"code": "x"}]),
        &schema,
    )
    .unwrap();

    let code = column(&decoded, "code").as_string::<i32>();
    let tags = column(&decoded, "tags").as_string::<i32>();

    assert_eq!(code.iter().collect::<Vec<_>>(), [Some("3"), Some("2.5")]);
    assert_eq!(
        tags.iter().collect::<Vec<_>>(),
        [Some(r#"["a",1]"#), Some("plain")]
    );
    assert_eq!(
        decoded.errors.len(),
        1,
        "a string is not a Float64 even when encrypted"
    );
}

#[test]
fn kept_rows_come_out_with_their_watched_fields() {
    let watched: HashSet<String> = ["level".to_owned(), "missing".to_owned()].into();
    let schema = Schema {
        fields: vec![field("level", FieldType::Utf8, false)],
    };

    let decoded = run_with(
        &json!([{"_timestamp": 9, "level": "error", "message": "x"}, {"message": "y"}, {"level": "info"}]),
        &schema,
        Options {
            masker: None,
            watched: Some(&watched),
        },
    )
    .unwrap();

    let events: Vec<_> = decoded
        .watched
        .iter()
        .map(|event| (event.timestamp.0, Json::Object(event.fields.clone())))
        .collect();

    assert_eq!(
        events,
        [
            (9, json!({"level": "error"})),
            (NOW.0, json!({"level": "info"}))
        ]
    );
}
