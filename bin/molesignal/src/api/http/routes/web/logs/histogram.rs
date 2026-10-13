// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How many log rows a structured query matches, bar by bar of the explorer
//! histogram.
//!
//! The list returns a page. This counts everything the same query matches in
//! its time range, selecting with the conditions the list itself selects by.

use axum::{Extension, Json, extract::State};
use serde::Deserialize;

use super::{
    LogFilter, TIMESTAMP_MICROS, clean_required, match_clauses, normalize_filters,
    normalize_free_text, validate_filter_fields,
};
use crate::{
    api::{
        AppState,
        http::routes::web::histogram::{
            BUCKET_COLUMN, BucketPlan, Counts, ERRORS_COLUMN, HistogramResponse, TOTAL_COLUMN,
            Unavailable, read_counts, within_budget,
        },
    },
    app::iam::IamContext,
    domain::{
        iam::permission,
        query::{QueryLanguage, QueryRequest, StreamHint},
        stream::{FieldDef, StreamType},
    },
    infra::query::escape_sql_ident,
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

/// The fields the level of a log is read from, in the order the explorer
/// reads them.
const LEVEL_FIELDS: [&str; 2] = ["level", "body_level"];

#[derive(Debug, Default, Deserialize)]
pub(super) struct LogHistogramRequest {
    stream: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    #[serde(default)]
    filters: Vec<LogFilter>,
    #[serde(default)]
    free_text: Vec<String>,
    bucket_micros: Option<i64>,
    origin_micros: Option<i64>,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
pub(super) async fn histogram(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Json(request): Json<LogHistogramRequest>,
) -> Result<Json<HistogramResponse>> {
    let plan = BucketPlan::new(
        request.from,
        request.to,
        request.bucket_micros,
        request.origin_micros,
    )?;

    let stream_name = clean_required(request.stream, "log stream is required", 128)?;
    let filters = normalize_filters(request.filters)?;
    let free_text = normalize_free_text(request.free_text)?;

    let stream = match state
        .telemetry
        .streams
        .get(&iam.org_id, &stream_name, StreamType::LOGS)
        .await
    {
        Ok(stream) => stream,
        // A stream nothing was written to matches nothing: its list is empty too.
        Err(Error::NotFound(_)) => return Ok(Json(plan.counted(Vec::new(), false))),
        Err(error) => return Err(error),
    };

    let fields = &stream.schema.fields;
    validate_filter_fields(&filters, &free_text, fields)?;

    let level = level_sql(fields);
    let statement = histogram_sql(
        &plan,
        &stream_name,
        &filters,
        &free_text,
        fields,
        level.as_deref(),
    )?;

    // The same path as a query a user writes: admitted by role, listed among
    // the running queries, masked, and cached once its time range is in the past.
    let count = state.query.run_tracked(
        QueryRequest {
            org_id: iam.org_id.clone(),
            language: QueryLanguage::Sql,
            statement,
            time_range: TimeRange::new(TimestampMicros(plan.from), TimestampMicros(plan.to)),
            stream: Some(StreamHint {
                name: stream_name,
                stream_type: StreamType::LOGS,
            }),
            limit: None,
            federation_clusters: Vec::new(),
        },
        iam.user_id.clone(),
        iam.organization_role_key(),
    );

    let output = match within_budget(count).await {
        Ok(Some(output)) => output,
        Ok(None) => return Ok(Json(plan.unavailable(Unavailable::TimeBudget))),
        Err(Error::ResourceExhausted(_)) => return Ok(Json(plan.unavailable(Unavailable::Busy))),
        Err(error) => return Err(error),
    };

    Ok(Json(match read_counts(&output.columns, &output.rows)? {
        Counts::Bars { bars, split } => plan.counted(bars, split && level.is_some()),
        Counts::TimeMasked => plan.unavailable(Unavailable::TimestampMasked),
    }))
}

/// SQL for the level of a row as text, read as the explorer reads it: the
/// first of the level fields the row has a value for. `None` when the stream
/// has neither field, and so no error to tell apart from the rest.
fn level_sql(fields: &[FieldDef]) -> Option<String> {
    let present = LEVEL_FIELDS
        .iter()
        .filter(|name| fields.iter().any(|field| field.name == **name))
        .map(|name| format!("CAST(\"{name}\" AS VARCHAR)"))
        .collect::<Vec<_>>();

    match present.as_slice() {
        [] => None,
        [only] => Some(only.clone()),
        several => Some(format!("COALESCE({})", several.join(", "))),
    }
}

/// The statement that counts the matching rows into the bars of `plan`, the
/// rows at level `ERROR` apart from the rest.
///
/// The error count is written over the level fields themselves, so that it
/// comes back masked when one of them is: a count must not show what the rows
/// would hide.
fn histogram_sql(
    plan: &BucketPlan,
    stream: &str,
    filters: &[LogFilter],
    free_text: &[String],
    fields: &[FieldDef],
    level: Option<&str>,
) -> Result<String> {
    let clauses = match_clauses((plan.from, plan.to), filters, free_text, fields)?;

    let errors = level
        .map(|level| {
            format!(
                ", SUM(CASE WHEN UPPER({level}) = 'ERROR' THEN 1 ELSE 0 END) AS {ERRORS_COLUMN}"
            )
        })
        .unwrap_or_default();

    Ok(format!(
        "SELECT {} AS {BUCKET_COLUMN}, COUNT(*) AS {TOTAL_COLUMN}{errors} \
         FROM \"{}\" \
         WHERE {} \
         GROUP BY 1",
        plan.bucket_sql(TIMESTAMP_MICROS),
        escape_sql_ident(stream),
        clauses.join(" AND "),
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{
        array::{RecordBatch, StringArray, TimestampMicrosecondArray},
        datatypes::{DataType, Field, Schema, TimeUnit},
    };
    use datafusion::datasource::MemTable;

    use super::*;
    use crate::{
        api::http::routes::web::histogram::BarCount,
        domain::stream::FieldType,
        infra::query::{analyzer::session_context, arrow_json::batches_to_json},
    };

    fn field(name: &str) -> FieldDef {
        FieldDef {
            name: name.into(),
            data_type: FieldType::Utf8,
            nullable: true,
            index_type: None,
            indexed: false,
            encrypted: false,
            exact: false,
        }
    }

    /// Bars of a millisecond from 10 ms, counting what lies in `[10.5 ms, 13 ms)`.
    fn plan() -> BucketPlan {
        BucketPlan::new(Some(10_500), Some(13_000), Some(1_000), Some(10_000)).expect("plan")
    }

    fn bar(index: i64, total: u64, errors: u64) -> BarCount {
        BarCount {
            index,
            total,
            errors,
        }
    }

    /// Seven rows around the range: one before it, one at its end, five inside.
    fn rows(columns: &[&str]) -> RecordBatch {
        let times = [10_400, 10_500, 10_999, 11_000, 12_100, 12_999, 13_000];
        let level = [
            Some("ERROR"),
            Some("ERROR"),
            Some("error"),
            Some("INFO"),
            None,
            Some("WARN"),
            Some("ERROR"),
        ];
        let body_level = [None, None, None, Some("ERROR"), Some("ERROR"), None, None];
        let service = ["api", "api", "web", "api", "api", "web", "api"];

        let mut fields = vec![Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        )];
        let mut arrays: Vec<arrow::array::ArrayRef> = vec![Arc::new(
            TimestampMicrosecondArray::from(times.to_vec()).with_timezone("UTC"),
        )];

        for column in columns {
            fields.push(Field::new(*column, DataType::Utf8, true));

            arrays.push(match *column {
                "level" => Arc::new(StringArray::from(level.to_vec())),
                "body_level" => Arc::new(StringArray::from(body_level.to_vec())),
                _ => Arc::new(StringArray::from(service.to_vec())),
            });
        }

        RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays).expect("record batch")
    }

    /// Runs a counting statement the way the engine plans one, and reads it back.
    async fn counted(statement: &str, batch: RecordBatch) -> Vec<BarCount> {
        let session = session_context(0);
        let table = MemTable::try_new(batch.schema(), vec![vec![batch]]).expect("mem table");
        session
            .register_table("app_logs", Arc::new(table))
            .expect("register table");

        let batches = session
            .sql(statement)
            .await
            .expect("the statement should plan")
            .collect()
            .await
            .expect("the statement should execute");
        let (columns, rows) = batches_to_json(&batches);

        match read_counts(&columns, &rows).expect("counts") {
            Counts::Bars { mut bars, .. } => {
                bars.sort_by_key(|bar| bar.index);
                bars
            }
            Counts::TimeMasked => panic!("time is not masked"),
        }
    }

    #[test]
    fn the_level_is_read_from_the_fields_the_stream_has() {
        assert_eq!(level_sql(&[field("service")]), None);
        assert_eq!(
            level_sql(&[field("level")]).as_deref(),
            Some("CAST(\"level\" AS VARCHAR)")
        );
        assert_eq!(
            level_sql(&[field("body_level"), field("level")]).as_deref(),
            Some("COALESCE(CAST(\"level\" AS VARCHAR), CAST(\"body_level\" AS VARCHAR))")
        );
    }

    #[test]
    fn the_statement_selects_with_the_conditions_of_the_list() {
        let fields = [field("service"), field("level"), field("message")];
        let filters = [LogFilter {
            field: "service".into(),
            op: "=".into(),
            value: "api'".into(),
            quoted: true,
        }];
        let free_text = ["timeout".to_string()];
        let level = level_sql(&fields);

        let statement = histogram_sql(
            &plan(),
            "app\"logs",
            &filters,
            &free_text,
            &fields,
            level.as_deref(),
        )
        .expect("sql");
        let clauses =
            match_clauses((10_500, 13_000), &filters, &free_text, &fields).expect("clauses");

        assert!(statement.contains(&format!("WHERE {} GROUP BY 1", clauses.join(" AND "))));
        assert!(statement.contains("CAST(_timestamp AS BIGINT) >= 10500"));
        assert!(statement.contains("CAST(_timestamp AS BIGINT) < 13000"));
        assert!(statement.contains("\"service\" = 'api'''"));
        assert!(statement.contains("MATCH(message, 'timeout')"));
        assert!(statement.contains("FROM \"app\"\"logs\""));
        assert!(statement.contains(
            "(CAST(_timestamp AS BIGINT) - 10000) / 1000 AS __histogram_bucket, COUNT(*) AS \
             __histogram_total"
        ));
    }

    #[tokio::test]
    async fn rows_are_counted_into_their_bars_with_errors_apart() {
        let fields = [field("level"), field("body_level"), field("service")];
        let level = level_sql(&fields);
        let statement =
            histogram_sql(&plan(), "app_logs", &[], &[], &fields, level.as_deref()).expect("sql");

        // The row before the range and the row at its end are not counted. A
        // level is compared whatever its case, and a row without a level is
        // read by its body level; a row with a level is not.
        assert_eq!(
            counted(&statement, rows(&["level", "body_level", "service"])).await,
            vec![bar(0, 2, 2), bar(1, 1, 0), bar(2, 2, 1)]
        );
    }

    #[tokio::test]
    async fn a_filter_narrows_what_is_counted() {
        let fields = [field("level"), field("service")];
        let filters = [LogFilter {
            field: "service".into(),
            op: "=".into(),
            value: "web".into(),
            quoted: true,
        }];
        let level = level_sql(&fields);
        let statement = histogram_sql(
            &plan(),
            "app_logs",
            &filters,
            &[],
            &fields,
            level.as_deref(),
        )
        .expect("sql");

        assert_eq!(
            counted(&statement, rows(&["level", "service"])).await,
            vec![bar(0, 1, 1), bar(2, 1, 0)]
        );
    }

    #[tokio::test]
    async fn a_stream_without_a_level_is_counted_whole() {
        let fields = [field("service")];
        let statement = histogram_sql(&plan(), "app_logs", &[], &[], &fields, None).expect("sql");
        assert!(!statement.contains(ERRORS_COLUMN));

        let session = session_context(0);
        let batch = rows(&["service"]);
        let table = MemTable::try_new(batch.schema(), vec![vec![batch]]).expect("mem table");
        session
            .register_table("app_logs", Arc::new(table))
            .expect("register table");

        let batches = session
            .sql(&statement)
            .await
            .expect("the statement should plan")
            .collect()
            .await
            .expect("the statement should execute");
        let (columns, rows) = batches_to_json(&batches);

        let Counts::Bars { mut bars, split } = read_counts(&columns, &rows).expect("counts") else {
            panic!("time is not masked");
        };
        bars.sort_by_key(|bar| bar.index);

        assert!(!split);
        assert_eq!(bars, vec![bar(0, 2, 0), bar(1, 1, 0), bar(2, 2, 0)]);
    }
}
