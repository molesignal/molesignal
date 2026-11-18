// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 按一列的值数行的 `GROUP BY`：整个落在时间窗里、每一行都有着落的文件由整值索引的词典数出，
//! 与 DataFusion 对其余文件与缓冲数出的组合起来，再照语句排序、截断。

use std::{cmp::Ordering, collections::HashMap, sync::Arc};

use arrow::{
    array::{ArrayRef, Int64Array, RecordBatch, StringArray},
    compute::cast,
    datatypes::{Schema, SchemaRef},
};
use domain::storage::QueryFile;
use kernel::{Error, Result};
use query_language::{GroupKey, GroupOrder, GroupedCounts};
use serde_json::Value;

use super::merge::{each_value, to_i64};

/// 一个组：值（没有值时为 `None`）与行数。
type Group = (Option<String>, u64);

/// 文件里 `column` 没有值的行数，只在每一行都有着落时给出：词典里数过的值之外，其余的行正好
/// 是写入时记下的没有值的行。剩下的行可能是空串，也可能是没能建索引的值，这样的文件照常扫描。
pub(super) fn missing_rows(
    file: &QueryFile,
    column: &str,
    values: &HashMap<String, u64>,
) -> Option<u64> {
    let present = values.values().copied().fold(0_u64, u64::saturating_add);
    let missing = file.rows.checked_sub(present)?;

    if missing == 0 {
        return Some(0);
    }

    let nulls = file.null_counts.get(column).and_then(Value::as_u64)?;

    (nulls == missing).then_some(missing)
}

/// 把索引数出的每个值与没有值的行数，与 DataFusion 对其余文件与缓冲数出的组相加，照语句的
/// ORDER BY 排序，取语句与请求的行数上限中较小的那个，得到一个批次。列的次序、名字与类型照
/// `schema`；有没有值的组时，值一列可以为空。
pub(super) fn add_groups(
    batches: &mut Vec<RecordBatch>,
    schema: SchemaRef,
    groups: &GroupedCounts,
    limit: Option<usize>,
    counted: &HashMap<String, u64>,
    nulls: u64,
) -> Result<()> {
    let (value_column, rows_column) = match groups.value_first {
        true => (0, 1),
        false => (1, 0),
    };

    let mut all = counted
        .iter()
        .map(|(value, count)| (Some(value.clone()), *count))
        .collect::<HashMap<_, _>>();

    if nulls > 0 {
        *all.entry(None).or_default() += nulls;
    }

    for batch in batches.iter() {
        each_value(batch, value_column, rows_column, |value, count| {
            *all.entry(value.map(str::to_owned)).or_default() += count;
        })?;
    }

    let mut ranked = all.into_iter().collect::<Vec<Group>>();
    ranked.sort_by(|left, right| compare(left, right, &groups.order));

    if let Some(limit) = [groups.limit, limit].into_iter().flatten().min() {
        ranked.truncate(limit);
    }

    let field = schema.field(value_column).clone().with_nullable(true);

    let values: ArrayRef = Arc::new(
        ranked
            .iter()
            .map(|(value, _)| value.as_deref())
            .collect::<StringArray>(),
    );

    let values = cast(&values, field.data_type())
        .map_err(|error| Error::internal(format!("group result: {error}")))?;

    let rows: ArrayRef = Arc::new(Int64Array::from_iter_values(
        ranked.iter().map(|(_, count)| to_i64(*count)),
    ));

    let mut fields = schema
        .fields()
        .iter()
        .map(|field| field.as_ref().clone())
        .collect::<Vec<_>>();

    fields[value_column] = field;

    let mut columns = vec![rows.clone(), rows];
    columns[value_column] = values;

    let schema = Arc::new(Schema::new_with_metadata(fields, schema.metadata().clone()));

    let merged = RecordBatch::try_new(schema, columns)
        .map_err(|error| Error::internal(format!("group result: {error}")))?;

    *batches = vec![merged];
    Ok(())
}

/// ORDER BY 各项依次比较两组；都相同时按值升序，结果的次序不随哈希表变化。
fn compare(left: &Group, right: &Group, order: &[GroupOrder]) -> Ordering {
    order
        .iter()
        .map(|key| match key.key {
            GroupKey::Rows => directed(left.1.cmp(&right.1), key.descending),
            GroupKey::Value => match (&left.0, &right.0) {
                (Some(left), Some(right)) => directed(left.cmp(right), key.descending),
                (None, None) => Ordering::Equal,
                (None, Some(_)) => directed(Ordering::Less, !key.nulls_first),
                (Some(_), None) => directed(Ordering::Greater, !key.nulls_first),
            },
        })
        .find(|ordering| ordering.is_ne())
        .unwrap_or_else(|| left.0.cmp(&right.0))
}

fn directed(ordering: Ordering, reversed: bool) -> Ordering {
    match reversed {
        true => ordering.reverse(),
        false => ordering,
    }
}

#[cfg(test)]
mod tests {
    use arrow::datatypes::{DataType, Field};
    use domain::{storage::primary_dataset_type, stream::StreamType};
    use kernel::{
        ids::Id,
        time::{TimeRange, TimestampMicros},
    };

    use super::*;

    fn order(key: GroupKey, descending: bool) -> GroupOrder {
        GroupOrder {
            key,
            descending,
            nulls_first: descending,
        }
    }

    fn groups(value_first: bool, order: Vec<GroupOrder>, limit: Option<usize>) -> GroupedCounts {
        GroupedCounts {
            column: "service".into(),
            value_first,
            from: None,
            to: None,
            filters: Vec::new(),
            order,
            limit,
            grouped: String::new(),
        }
    }

    /// DataFusion 对其余文件的结果：值一列（不可为空）、行数一列，`value_first` 时值在前。
    fn scanned(value_first: bool, rows: &[(&str, i64)]) -> (SchemaRef, RecordBatch) {
        let value = Field::new("service", DataType::Utf8View, false);
        let count = Field::new("n", DataType::Int64, false);

        let values: ArrayRef = Arc::new(
            rows.iter()
                .map(|(value, _)| Some(*value))
                .collect::<arrow::array::StringViewArray>(),
        );

        let counts: ArrayRef = Arc::new(Int64Array::from_iter_values(
            rows.iter().map(|(_, count)| *count),
        ));

        let (fields, columns) = match value_first {
            true => (vec![value, count], vec![values, counts]),
            false => (vec![count, value], vec![counts, values]),
        };

        let schema = Arc::new(Schema::new(fields));
        let batch = RecordBatch::try_new(schema.clone(), columns).unwrap();

        (schema, batch)
    }

    fn rows(batch: &RecordBatch, value_first: bool) -> Vec<(Option<String>, i64)> {
        let (value_column, rows_column) = match value_first {
            true => (0, 1),
            false => (1, 0),
        };

        let mut out = Vec::new();

        each_value(batch, value_column, rows_column, |value, count| {
            out.push((value.map(str::to_owned), count as i64));
        })
        .unwrap();

        out
    }

    #[test]
    fn counted_and_scanned_groups_add_up_in_the_statement_order() {
        let (schema, batch) = scanned(true, &[("b", 2), ("c", 1)]);
        let mut batches = vec![batch];
        let counted = HashMap::from([("a".to_owned(), 1), ("b".to_owned(), 3)]);

        let by_rows = groups(
            true,
            vec![order(GroupKey::Rows, true), order(GroupKey::Value, false)],
            None,
        );

        add_groups(&mut batches, schema, &by_rows, None, &counted, 1).unwrap();

        // 同样多时按值升序，没有值的排在最后。
        assert_eq!(
            rows(&batches[0], true),
            vec![
                (Some("b".to_owned()), 5),
                (Some("a".to_owned()), 1),
                (Some("c".to_owned()), 1),
                (None, 1),
            ]
        );
        assert!(batches[0].schema().field(0).is_nullable());
    }

    #[test]
    fn the_smaller_limit_applies_and_the_columns_keep_their_places() {
        let (schema, batch) = scanned(false, &[("b", 2)]);
        let mut batches = vec![batch];
        let counted = HashMap::from([("a".to_owned(), 4), ("c".to_owned(), 1)]);
        let by_value = groups(false, vec![order(GroupKey::Value, true)], Some(2));

        add_groups(
            &mut batches,
            schema.clone(),
            &by_value,
            Some(5),
            &counted,
            0,
        )
        .unwrap();

        assert_eq!(
            rows(&batches[0], false),
            vec![(Some("c".to_owned()), 1), (Some("b".to_owned()), 2)]
        );
        assert_eq!(batches[0].schema().field(0).name(), "n");
        assert_eq!(
            batches[0].schema().field(1).data_type(),
            &DataType::Utf8View
        );

        let mut batches = Vec::new();
        let ascending = groups(false, vec![order(GroupKey::Value, false)], None);

        add_groups(&mut batches, schema, &ascending, Some(1), &counted, 3).unwrap();

        assert_eq!(rows(&batches[0], false), vec![(Some("a".to_owned()), 4)]);
    }

    fn file(rows: u64, nulls: Option<u64>) -> QueryFile {
        QueryFile {
            id: Id::from_string("file"),
            org_id: Id::from_string("org"),
            stream: "logs".into(),
            stream_type: StreamType::LOGS,
            dataset_type: primary_dataset_type(StreamType::LOGS).unwrap(),
            object_key: "objects/file".into(),
            checksum: None,
            etag: None,
            time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(1)),
            rows,
            size_bytes: rows,
            min_values: Default::default(),
            max_values: Default::default(),
            value_sets: Default::default(),
            address_ranges: Default::default(),
            null_counts: nulls
                .map(|nulls| serde_json::Map::from_iter([("service".to_owned(), nulls.into())]))
                .unwrap_or_default(),
            indexed_at_micros: None,
            shard: None,
            clustering: Default::default(),
        }
    }

    #[test]
    fn a_file_is_counted_only_when_every_row_is_accounted_for() {
        let values = HashMap::from([("a".to_owned(), 3), ("b".to_owned(), 2)]);

        assert_eq!(missing_rows(&file(5, None), "service", &values), Some(0));
        assert_eq!(missing_rows(&file(7, Some(2)), "service", &values), Some(2));

        // 两行没有着落，只记下一行没有值：另一行可能是空串。
        assert_eq!(missing_rows(&file(7, Some(1)), "service", &values), None);
        assert_eq!(missing_rows(&file(7, None), "service", &values), None);
        assert_eq!(missing_rows(&file(4, None), "service", &values), None);
    }
}
