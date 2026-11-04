// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::{Value, json};

use super::*;
use crate::{
    domain::stream::{FieldDef, FieldType, StreamIndexType, address_key, address_number},
    infra::query::refutations::Refutations,
    shared::{
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

fn schema(index_type: StreamIndexType, data_type: FieldType) -> Schema {
    let mut field = FieldDef {
        name: "value".into(),
        data_type,
        nullable: true,
        index_type: None,
        indexed: false,
        encrypted: false,
        exact: false,
    };

    field.configure_index(true, index_type);

    Schema {
        fields: vec![field],
    }
}

fn file(id: &str, minimum: Value, maximum: Value) -> QueryFile {
    QueryFile {
        id: Id::from_string(id),
        org_id: Id::from_string("org"),
        stream: "logs".into(),
        stream_type: crate::domain::stream::StreamType::LOGS,
        dataset_type: crate::domain::storage::primary_dataset_type(
            crate::domain::stream::StreamType::LOGS,
        )
        .unwrap(),
        object_key: format!("{id}.parquet"),
        checksum: None,
        etag: None,
        time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(1)),
        rows: 1,
        size_bytes: 1,
        min_values: [("value".into(), minimum)].into_iter().collect(),
        max_values: [("value".into(), maximum)].into_iter().collect(),
        value_sets: serde_json::Map::new(),
        address_ranges: serde_json::Map::new(),
        null_counts: serde_json::Map::new(),
        indexed_at_micros: None,
        shard: None,
    }
}

/// 只用文件统计剪枝之后留下的文件。
fn prune(files: Vec<QueryFile>, statement: &str, schema: &Schema) -> Vec<QueryFile> {
    let tree = WhereTree::parse(statement);
    let atoms = Atoms::new(&tree, schema);

    Refutations::new(tree).retain(files, |file| atoms.refuted(file))
}

fn kept(files: &[QueryFile], statement: &str, schema: &Schema) -> Vec<String> {
    prune(files.to_vec(), statement, schema)
        .iter()
        .map(|file| file.id.as_str().to_string())
        .collect()
}

fn two_files() -> Vec<QueryFile> {
    vec![
        file("low", json!(10), json!(20)),
        file("high", json!(30), json!(40)),
    ]
}

#[test]
fn numeric_comparisons_prune_only_proven_misses() {
    let schema = schema(StreamIndexType::Skip, FieldType::Int64);

    assert_eq!(
        kept(&two_files(), "SELECT * FROM logs WHERE value > 25", &schema),
        ["high"]
    );
    assert_eq!(
        kept(&two_files(), "SELECT * FROM logs WHERE value = 20", &schema),
        ["low"]
    );
    assert_eq!(
        kept(&two_files(), "SELECT * FROM logs WHERE 25 > value", &schema),
        ["low"]
    );
}

/// 每种索引的字段都记了 min/max，都拿来裁剪。
#[test]
fn bounds_of_every_indexed_field_prune() {
    for index_type in [
        StreamIndexType::Exact,
        StreamIndexType::Bloom,
        StreamIndexType::Skip,
    ] {
        assert_eq!(
            kept(
                &two_files(),
                "SELECT * FROM logs WHERE value > 100",
                &schema(index_type, FieldType::Int64)
            ),
            Vec::<String>::new(),
            "{index_type:?}"
        );
    }
}

#[test]
fn in_lists_between_and_not_in_use_the_bounds() {
    let schema = schema(StreamIndexType::Skip, FieldType::Int64);

    assert_eq!(
        kept(
            &two_files(),
            "SELECT * FROM logs WHERE value IN (15, 50)",
            &schema
        ),
        ["low"]
    );
    assert_eq!(
        kept(
            &two_files(),
            "SELECT * FROM logs WHERE value BETWEEN 21 AND 29",
            &schema
        ),
        Vec::<String>::new()
    );

    let single = vec![
        file("single", json!(7), json!(7)),
        file("pair", json!(7), json!(8)),
    ];

    assert_eq!(
        kept(
            &single,
            "SELECT * FROM logs WHERE value NOT IN (7, 9)",
            &schema
        ),
        ["pair"]
    );
    assert_eq!(
        kept(&single, "SELECT * FROM logs WHERE value != 7", &schema),
        ["pair"]
    );
}

/// OR 的每一支都被证明不成立才裁剪：有一支证明不了（不在统计里的列、NOT）就不裁。
#[test]
fn an_or_prunes_only_when_every_branch_is_refuted() {
    let schema = schema(StreamIndexType::Skip, FieldType::Int64);
    let files = two_files();

    assert_eq!(
        kept(
            &files,
            "SELECT * FROM logs WHERE value > 100 OR (value < 5)",
            &schema
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        kept(
            &files,
            "SELECT * FROM logs WHERE value = 15 OR value IN (35, 36)",
            &schema
        ),
        ["low", "high"]
    );
    assert_eq!(
        kept(
            &files,
            "SELECT * FROM logs WHERE CAST(_timestamp AS BIGINT) >= 1 \
             AND (value > 100 OR other = 1)",
            &schema
        ),
        ["low", "high"]
    );
    assert_eq!(
        kept(
            &files,
            "SELECT * FROM logs WHERE value > 25 AND ((value < 5) OR other = 1)",
            &schema
        ),
        ["high"]
    );
    assert_eq!(
        kept(&files, "SELECT * FROM logs WHERE NOT (value > 25)", &schema),
        ["low", "high"]
    );
}

/// 落在不同列上的 OR：每一支各由自己那一列的统计证明。
#[test]
fn an_or_across_columns_prunes_files_where_every_column_rules_its_branch_out() {
    let mut schema = schema(StreamIndexType::Skip, FieldType::Int64);
    let mut other = schema.fields[0].clone();
    other.name = "other".into();
    schema.fields.push(other);

    let with_other = |id: &str, value: (i64, i64), other: (i64, i64)| {
        let mut file = file(id, json!(value.0), json!(value.1));
        file.min_values.insert("other".into(), json!(other.0));
        file.max_values.insert("other".into(), json!(other.1));
        file
    };

    let files = vec![
        with_other("neither", (10, 20), (1, 2)),
        with_other("value", (90, 120), (1, 2)),
        with_other("other", (10, 20), (60, 70)),
    ];

    assert_eq!(
        kept(
            &files,
            "SELECT * FROM logs WHERE value > 100 OR other >= 50",
            &schema
        ),
        ["value", "other"]
    );
    assert_eq!(
        kept(
            &files,
            "SELECT * FROM logs WHERE value < 5 OR (value > 15 AND other > 65)",
            &schema
        ),
        ["other"]
    );
}

#[test]
fn string_boolean_and_cast_timestamp_bounds_are_supported() {
    assert!(
        prune(
            vec![file("string", json!("api"), json!("web"))],
            "SELECT * FROM logs WHERE value = 'zzz'",
            &schema(StreamIndexType::Skip, FieldType::Utf8),
        )
        .is_empty()
    );
    assert!(
        prune(
            vec![file("bool", json!(false), json!(false))],
            "SELECT * FROM logs WHERE value = TRUE",
            &schema(StreamIndexType::Skip, FieldType::Bool),
        )
        .is_empty()
    );
    assert!(
        prune(
            vec![file("timestamp", json!(100), json!(200))],
            "SELECT * FROM logs WHERE CAST(value AS BIGINT) >= 300",
            &schema(StreamIndexType::Skip, FieldType::Timestamp),
        )
        .is_empty()
    );
}

/// 前缀 `LIKE`：以它开头的字符串要落在 [min, max] 里；截短进位过的 max 照样判断。
#[test]
fn a_like_prefix_uses_the_string_bounds() {
    let schema = schema(StreamIndexType::Exact, FieldType::Utf8);
    let files = vec![
        file("api", json!("api-a"), json!("api-z")),
        file("web", json!("web-1"), json!("web-9")),
        file("wide", json!("aa"), json!("zz")),
    ];

    assert_eq!(
        kept(
            &files,
            "SELECT * FROM logs WHERE value LIKE 'web-%'",
            &schema
        ),
        ["web", "wide"]
    );
    assert_eq!(
        kept(
            &files,
            "SELECT * FROM logs WHERE CAST(value AS VARCHAR) LIKE 'api-q%x'",
            &schema
        ),
        ["api", "wide"]
    );
    assert_eq!(
        kept(
            &files,
            "SELECT * FROM logs WHERE value LIKE '%web%'",
            &schema
        ),
        ["api", "web", "wide"],
        "a pattern that starts with a wildcard has no prefix"
    );
    assert_eq!(
        kept(
            &files,
            "SELECT * FROM logs WHERE value ILIKE 'web-%'",
            &schema
        ),
        ["api", "web", "wide"],
        "ILIKE ignores case, so the bounds cannot tell"
    );
}

/// 值集合是段里出现过的全部值：落在 min/max 之间、却不在集合里的值也能排除。
#[test]
fn value_sets_prune_values_inside_the_bounds() {
    let schema = schema(StreamIndexType::Skip, FieldType::Utf8);
    let mut levels = file("levels", json!("debug"), json!("warn"));
    levels
        .value_sets
        .insert("value".into(), json!(["debug", "warn"]));
    let files = vec![levels];

    for (statement, expected) in [
        ("SELECT * FROM logs WHERE value = 'info'", 0),
        ("SELECT * FROM logs WHERE value IN ('info', 'warn')", 1),
        ("SELECT * FROM logs WHERE value NOT IN ('debug', 'warn')", 0),
        ("SELECT * FROM logs WHERE value != 'debug'", 1),
        ("SELECT * FROM logs WHERE value LIKE 'err%'", 0),
        ("SELECT * FROM logs WHERE value BETWEEN 'e' AND 'v'", 0),
    ] {
        assert_eq!(
            prune(files.clone(), statement, &schema).len(),
            expected,
            "{statement}"
        );
    }
}

#[test]
fn address_ranges_answer_networks_and_addresses() {
    let ip_schema = schema(StreamIndexType::Ip, FieldType::Utf8);
    let key = |text: &str| address_key(address_number(text).unwrap());
    let mut clients = file("clients", Value::Null, Value::Null);
    clients.min_values.clear();
    clients.max_values.clear();
    clients
        .address_ranges
        .insert("value".into(), json!([key("10.0.0.1"), key("10.0.0.9")]));
    let files = vec![clients];

    for (statement, expected) in [
        (
            "SELECT * FROM logs WHERE cidr_match(value, '10.0.0.0/8')",
            1,
        ),
        (
            "SELECT * FROM logs WHERE CIDR_MATCH(value, '192.168.0.0/16')",
            0,
        ),
        ("SELECT * FROM logs WHERE value = '10.0.0.5'", 1),
        (
            "SELECT * FROM logs WHERE value IN ('10.0.1.5', '10.0.2.5')",
            0,
        ),
        ("SELECT * FROM logs WHERE value = 'unknown'", 1),
        (
            "SELECT * FROM logs WHERE cidr_match(value, 'not a network')",
            1,
        ),
    ] {
        assert_eq!(
            prune(files.clone(), statement, &ip_schema).len(),
            expected,
            "{statement}"
        );
    }

    assert_eq!(
        prune(
            files,
            "SELECT * FROM logs WHERE cidr_match(value, '192.168.0.0/16')",
            &schema(StreamIndexType::Exact, FieldType::Utf8),
        )
        .len(),
        1,
        "only ip fields have address ranges"
    );
}

/// 取整的转换与统计对不上：`CAST(-5.5 AS BIGINT)` 是 -5，范围 [-5.9, -5.5] 的文件里有满足
/// `>= -5` 的行。连接与子查询里的列也不用来裁剪。
#[test]
fn rounding_casts_joins_and_derived_tables_never_prune() {
    let files = vec![file("negative", json!(-5.9), json!(-5.5))];
    let schema = schema(StreamIndexType::Skip, FieldType::Float64);

    for statement in [
        "SELECT * FROM logs WHERE CAST(value AS BIGINT) >= -5",
        "SELECT * FROM logs JOIN other ON logs.id = other.id WHERE value > 100",
        "SELECT * FROM (SELECT value * 100 AS value FROM logs) WHERE value > 100",
    ] {
        assert_eq!(
            prune(files.clone(), statement, &schema).len(),
            1,
            "{statement}"
        );
    }

    assert!(prune(files, "SELECT * FROM logs WHERE value > 100", &schema).is_empty());
}

#[test]
fn missing_bounds_and_qualified_columns_degrade_to_keep() {
    let mut missing = file("missing", json!(1), json!(2));
    missing.max_values.clear();
    let schema = schema(StreamIndexType::Skip, FieldType::Int64);

    assert_eq!(
        prune(
            vec![missing],
            "SELECT * FROM logs WHERE value > 100",
            &schema
        )
        .len(),
        1
    );
    assert_eq!(
        prune(
            vec![file("qualified", json!(1), json!(2))],
            "SELECT * FROM logs l WHERE l.value > 100",
            &schema,
        )
        .len(),
        1
    );
}

#[test]
fn encrypted_fields_and_uncertain_predicates_are_never_used_for_skipping() {
    let files = vec![file("encrypted", json!(1), json!(2))];
    let mut encrypted = schema(StreamIndexType::Skip, FieldType::Int64);
    encrypted.fields[0].encrypted = true;

    assert_eq!(
        prune(
            files.clone(),
            "SELECT * FROM logs WHERE value > 100",
            &encrypted
        )
        .len(),
        1
    );
    assert_eq!(
        prune(
            files,
            "SELECT * FROM logs WHERE value + 1 > 100",
            &schema(StreamIndexType::Skip, FieldType::Int64),
        )
        .len(),
        1
    );
}

/// 空值数：`IS NULL` 要有空的行，别的条件都要字段有值；全空的字段只可能满足 `IS NULL`。日志筛选
/// 的「有值」（`!= ''`、`IS NOT NULL`）因此跳过这个字段全空的文件。没有空值数的文件都保留。
#[test]
fn null_counts_answer_presence_filters() {
    let schema = schema(StreamIndexType::Skip, FieldType::Utf8);

    let counted = |id: &str, nulls: u64| {
        let mut file = file(id, json!("a"), json!("z"));
        file.rows = 10;
        file.null_counts.insert("value".into(), json!(nulls));
        file
    };

    let mut empty = counted("empty", 10);
    empty.min_values.clear();
    empty.max_values.clear();

    let mut unknown = file("unknown", json!("a"), json!("z"));
    unknown.rows = 10;

    let files = vec![empty, counted("full", 0), counted("some", 4), unknown];

    for (statement, expected) in [
        (
            "SELECT * FROM logs WHERE value IS NOT NULL",
            &["full", "some", "unknown"][..],
        ),
        (
            "SELECT * FROM logs WHERE value IS NULL",
            &["empty", "some", "unknown"],
        ),
        (
            "SELECT * FROM logs WHERE \"value\" != ''",
            &["full", "some", "unknown"],
        ),
        (
            "SELECT * FROM logs WHERE value = 'm'",
            &["full", "some", "unknown"],
        ),
        (
            "SELECT * FROM logs WHERE (\"value\" IS NULL OR \"value\" = '')",
            &["empty", "some", "unknown"],
        ),
    ] {
        assert_eq!(kept(&files, statement, &schema), expected, "{statement}");
    }
}
