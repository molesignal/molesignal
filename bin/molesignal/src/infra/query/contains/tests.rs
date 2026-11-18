// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use arrow::{
    array::{Int64Array, RecordBatch, StringArray},
    datatypes::{Field, Schema},
    util::pretty::pretty_format_batches,
};
use datafusion::{datasource::MemTable, prelude::SessionContext};

use super::*;
use crate::infra::query::analyzer::session_context;

fn table() -> Arc<MemTable> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("message", DataType::Utf8, true),
    ]));

    let messages = [
        Some("request needle0001 done"),
        Some("NEEDLE0001 shouted"),
        Some("ends with need"),
        Some("le starts here"),
        None,
        Some("调用完成 needle0010"),
        Some("under_score 100%"),
        Some("back\\slash"),
    ];

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(Int64Array::from_iter_values(0..messages.len() as i64)),
            Arc::new(StringArray::from(messages.to_vec())),
        ],
    )
    .unwrap();

    Arc::new(MemTable::try_new(schema, vec![vec![batch]]).unwrap())
}

async fn rows(ctx: &SessionContext, condition: &str) -> String {
    let frame = ctx
        .sql(&format!(
            "SELECT id FROM logs WHERE {condition} ORDER BY id"
        ))
        .await
        .unwrap();

    pretty_format_batches(&frame.collect().await.unwrap())
        .unwrap()
        .to_string()
}

async fn plan(ctx: &SessionContext, condition: &str) -> String {
    let frame = ctx
        .sql(&format!("SELECT id FROM logs WHERE {condition}"))
        .await
        .unwrap();

    format!("{}", frame.into_optimized_plan().unwrap().display_indent())
}

/// Every condition answers as plain DataFusion answers it; the substring ones become one call.
#[tokio::test]
async fn substring_likes_are_rewritten_and_answer_the_same() {
    let rewritten = session_context(0);
    let plain = SessionContext::new();

    rewritten.register_table("logs", table()).unwrap();
    plain.register_table("logs", table()).unwrap();

    let cases = [
        ("message LIKE '%needle0001%'", true),
        ("message NOT LIKE '%needle0001%'", true),
        ("message ILIKE '%needle0001%'", true),
        (
            "message LIKE '%needle0001%' OR message LIKE '%needle0010%'",
            true,
        ),
        ("message ILIKE '%NEEDLE%' OR message ILIKE '%slash%'", true),
        ("message LIKE '%调用完成%'", true),
        ("message LIKE '%needle%' OR id = 6", true),
        ("message LIKE 'request%'", false),
        ("message LIKE '%need_e%'", false),
        ("message LIKE '%100\\%%'", false),
        ("message ILIKE '%调用%'", false),
    ];

    for (condition, rewrites) in cases {
        assert_eq!(
            rows(&rewritten, condition).await,
            rows(&plain, condition).await,
            "{condition}"
        );

        let plan = plan(&rewritten, condition).await;
        assert_eq!(plan.contains(NAME), rewrites, "{condition}: {plan}");
    }
}

#[tokio::test]
async fn an_or_of_substrings_on_one_column_is_one_call() {
    let ctx = session_context(0);
    ctx.register_table("logs", table()).unwrap();

    let plan = plan(
        &ctx,
        "message LIKE '%a%' OR message LIKE '%b%' OR message LIKE '%c%'",
    )
    .await;

    assert_eq!(plan.matches(NAME).count(), 1, "{plan}");
}
