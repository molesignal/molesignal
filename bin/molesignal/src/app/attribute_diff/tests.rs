// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use domain::{
    query::QueryResult,
    stream::{StreamDefinition, StreamType},
};
use kernel::{ids::Id, time::TimestampMicros};
use serde_json::{Value, json};
use signals::trace::normalization::canonical_trace_schema;

use super::{spans::statement, *};

fn rows(values: &[(&str, Option<&str>)], times: usize) -> Rows {
    values
        .iter()
        .flat_map(|(region, tenant)| {
            std::iter::repeat_n(
                vec![Some(region.to_string()), tenant.map(str::to_string)],
                times,
            )
        })
        .collect()
}

#[test]
fn the_field_whose_values_the_chosen_set_has_apart_from_the_rest_comes_first() {
    let fields = vec!["region".to_string(), "tenant".to_string()];
    // The failed requests are nearly all of tenant acme; regions are mixed alike in both sets.
    let selected = rows(
        &[("eu", Some("acme")), ("us", Some("acme")), ("eu", None)],
        3,
    );
    let baseline = rows(
        &[
            ("eu", Some("globex")),
            ("us", Some("acme")),
            ("eu", Some("initech")),
        ],
        3,
    );

    let comparison = compare(&fields, &selected, &baseline, false);

    assert_eq!(comparison.selected, 9);
    assert_eq!(comparison.baseline, 9);
    assert_eq!(comparison.dimensions.len(), 1, "regions are shared alike");

    let tenant = &comparison.dimensions[0];
    assert_eq!(tenant.field, "tenant");
    assert!((tenant.score - 2.0 / 3.0).abs() < 1e-9, "{}", tenant.score);
    assert_eq!(tenant.values[0].value.as_deref(), Some("acme"));
    assert!((tenant.values[0].selected - 2.0 / 3.0).abs() < 1e-9);
    assert!((tenant.values[0].baseline - 1.0 / 3.0).abs() < 1e-9);
    assert!(
        tenant
            .values
            .iter()
            .any(|share| share.value.is_none() && share.baseline == 0.0),
        "the rows without a value are a value of their own"
    );
}

#[test]
fn an_id_like_field_is_left_out_and_an_empty_set_compares_nothing() {
    let fields = vec!["request_id".to_string()];
    let distinct = |prefix: &str| -> Rows {
        (0..15)
            .map(|index| vec![Some(format!("{prefix}{index}"))])
            .collect()
    };

    let comparison = compare(&fields, &distinct("a"), &distinct("b"), true);
    assert!(comparison.dimensions.is_empty());
    assert!(comparison.sampled);

    let empty = compare(&fields, &Vec::new(), &distinct("b"), false);
    assert!(empty.dimensions.is_empty());
}

fn definition() -> StreamDefinition {
    StreamDefinition {
        id: Id::from_string("spans"),
        org_id: Id::from_string("org"),
        name: "default".into(),
        stream_type: StreamType::TRACES,
        schema: canonical_trace_schema(),
        retention: None,
        created_at: TimestampMicros(0),
        updated_at: TimestampMicros(0),
    }
}

#[test]
fn ids_moments_and_documents_are_not_compared() {
    let definition = definition();
    let names = candidates(&definition.schema)
        .into_iter()
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>();

    assert!(names.contains(&"service.name"));
    assert!(names.contains(&"name"));
    assert!(names.contains(&"status_code"));

    for left_out in [
        "trace_id",
        "span_id",
        "duration_ns",
        "attributes",
        "resource",
    ] {
        assert!(!names.contains(&left_out), "{left_out}");
    }

    assert!(!names.iter().any(|name| name.starts_with("molesignal.")));
}

#[test]
fn the_chosen_requests_are_slow_failed_or_both() {
    let schema = definition().schema;

    let slow = Selection {
        min_duration: Some(500),
        max_duration: Some(900),
        errors: false,
    };
    assert_eq!(
        slow.clause(&schema).unwrap(),
        "(\"duration_ns\" >= 500 AND \"duration_ns\" < 900)"
    );

    let failed = Selection {
        errors: true,
        ..Selection::default()
    };
    assert!(failed.clause(&schema).unwrap().contains("= 'ERROR'"));

    assert!(Selection::default().clause(&schema).is_err());
    assert!(
        Selection {
            min_duration: Some(9),
            max_duration: Some(9),
            errors: false,
        }
        .clause(&schema)
        .is_err()
    );
}

#[test]
fn each_set_is_read_with_its_values_named_apart_from_every_field() {
    let definition = definition();
    let fields = candidates(&definition.schema);

    let read = statement(
        "default",
        &fields[..1],
        ["\"service.name\" = 'api'".to_string()].into_iter(),
        50,
    );

    assert!(read.starts_with("SELECT NULLIF(CAST("), "{read}");
    assert!(read.contains("AS __diff_0 FROM \"default\" WHERE \"service.name\" = 'api' LIMIT 50"));
}

#[tokio::test]
async fn both_sets_are_read_and_the_rest_is_what_is_not_chosen() {
    let definition = definition();
    let selection = Selection {
        errors: true,
        ..Selection::default()
    };
    let asked = SpanComparison {
        definition: &definition,
        clauses: vec!["\"service.name\" = 'api'".into()],
        selection: &selection,
        limit: 2,
    };

    let read = std::sync::Mutex::new(Vec::<String>::new());

    let comparison = compare_spans(asked, |statement: String| {
        let rest = statement.contains("NOT COALESCE(");
        read.lock().unwrap().push(statement);

        async move {
            let value = if rest { "200" } else { "500" };
            Ok(QueryResult {
                columns: Vec::new(),
                rows: vec![
                    vec![Value::Null, json!(value)],
                    vec![Value::Null, json!(value)],
                ],
                scanned_rows: 0,
                took_ms: 0,
                federation: None,
            })
        }
    })
    .await
    .unwrap();

    assert_eq!(read.lock().unwrap().len(), 2);
    assert!(comparison.sampled, "both sets filled the limit");
    assert_eq!(comparison.dimensions.len(), 1);
    assert_eq!(comparison.dimensions[0].score, 1.0);
}
