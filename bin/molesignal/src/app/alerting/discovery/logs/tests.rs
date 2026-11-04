// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::*;
use crate::domain::stream::{FieldDef, FieldType, Schema};

fn stream(name: &str, fields: &[&str]) -> StreamDefinition {
    StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string("org"),
        name: name.into(),
        stream_type: StreamType::LOGS,
        schema: Schema {
            fields: fields
                .iter()
                .map(|name| FieldDef {
                    name: name.to_string(),
                    data_type: FieldType::Utf8,
                    nullable: true,
                    index_type: None,
                    indexed: false,
                    encrypted: false,
                    exact: false,
                })
                .collect(),
        },
        retention: None,
        created_at: TimestampMicros(0),
        updated_at: TimestampMicros(0),
    }
}

fn result(columns: &[&str], rows: Vec<Vec<Value>>) -> QueryResult {
    QueryResult {
        columns: columns.iter().map(|column| column.to_string()).collect(),
        rows,
        scanned_rows: 0,
        took_ms: 0,
        federation: None,
    }
}

#[test]
fn only_log_streams_with_a_level_are_read_and_their_fields_are_found() {
    let shape = LogShape::of(&stream("app", &["level", "message", "service.name"])).unwrap();

    assert_eq!(shape.message.as_deref(), Some("message"));
    assert_eq!(shape.service.as_deref(), Some("service.name"));
    assert!(
        shape
            .surge_sql(100)
            .contains("FROM \"app\" WHERE lower(CAST(\"level\" AS VARCHAR)) IN ('error'")
    );
    assert!(
        shape
            .surge_sql(100)
            .contains("GROUP BY CAST(\"service.name\" AS VARCHAR)")
    );
    assert!(
        shape
            .new_errors_sql(100)
            .unwrap()
            .contains("HAVING MIN(CAST(_timestamp AS BIGINT)) >= 100")
    );

    let numbered = LogShape::of(&stream("otel", &["severity_number", "body"])).unwrap();
    assert_eq!(numbered.errors, "\"severity_number\" >= 17");
    assert!(
        numbered
            .surge_sql(1)
            .starts_with("SELECT 'otel' AS service")
    );

    assert!(LogShape::of(&stream("plain", &["message"])).is_none());
    assert!(LogShape::of(&stream("_sys_audit", &["level"])).is_none());
}

#[test]
fn a_surge_is_many_more_errors_than_the_hour_before() {
    let shape = LogShape::of(&stream("app", &["level", "message", "service.name"])).unwrap();
    let found = surges(
        &shape,
        &result(
            &["service", "recent", "earlier"],
            vec![
                vec![json!("checkout"), json!(120), json!(40)],
                vec![json!("search"), json!(30), json!(80)],
                vec![json!("quiet"), json!(5), json!(0)],
            ],
        ),
        TimestampMicros(10 * 60 * MINUTE),
    );

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].key, "log_error_surge|app|checkout");
    assert_eq!(found[0].severity, Severity::Error);
    assert_eq!(found[0].baseline, Some(10.0));
}

#[test]
fn new_error_templates_become_findings_keyed_by_their_text() {
    let shape = LogShape::of(&stream("app", &["level", "message"])).unwrap();
    let found = fresh_errors(
        &shape,
        &result(
            &["service", "template", "first_seen", "hits"],
            vec![
                vec![
                    json!("app"),
                    json!("timeout calling # after #ms"),
                    json!(500),
                    json!(7),
                ],
                vec![json!("app"), json!("rare"), json!(600), json!(1)],
            ],
        ),
    );

    assert_eq!(found.len(), 1);
    assert!(found[0].key.starts_with("log_new_error|app|app|"));
    assert_eq!(found[0].started_at, Some(TimestampMicros(500)));
    assert_eq!(found[0].detail["template"], "timeout calling # after #ms");
}
