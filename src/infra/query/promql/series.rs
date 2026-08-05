// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;
use crate::domain::metrics::{METRIC_NAME_FIELD, is_metric_identity_storage_field};

/// 把 record batches 按 (matchers 过滤) + (labels 分组) 转 Vec<Series>。
///
/// 热路径：每行只做「时间裁剪 → matcher 直读列值 → series key 查表」三件事，
/// 被过滤掉的行不构建 LabelSet；命中行也只在首次见到该 label 组合时构建一次。
/// 物化样本总数超过 `max_samples` 时直接报错（matrix 上限），提示收窄窗口。
pub(super) fn batches_to_series(
    batches: &[RecordBatch],
    matchers: &Matchers,
    logical_metric: Option<&str>,
    start_us: i64,
    end_us: i64,
    max_samples: usize,
) -> Result<Vec<Series>> {
    let mut by_key: HashMap<String, usize> = HashMap::new();
    let mut series: Vec<Series> = Vec::new();
    let mut total = 0usize;
    let mut key_buf = String::new();

    for b in batches {
        let schema = b.schema();
        let (Ok(ts_idx), Ok(value_idx)) = (schema.index_of("_timestamp"), schema.index_of("value"))
        else {
            continue;
        };
        let ts_arr = b
            .column(ts_idx)
            .as_any()
            .downcast_ref::<TimestampMicrosecondArray>();
        // `value` 列约定是 Float64，但整数值 metric（例如 JSON ingest `value: 1` 被
        // 推断成 Int64，或 OTLP 整数 sum/gauge）会落成 Int64/其它数值类型。用 arrow
        // cast 统一转 Float64，避免非 Float64 的 value 列被整批跳过导致图表空白。
        let value_f64 = arrow::compute::cast(b.column(value_idx), &DataType::Float64).ok();
        let (Some(ts_arr), Some(value_f64)) = (ts_arr, value_f64) else {
            continue;
        };
        let Some(val_arr) = value_f64.as_any().downcast_ref::<Float64Array>() else {
            continue;
        };
        // label 列（非 reserved 的 Utf8 列）一个 batch 解析一次；按列名排序，
        // 使 series key 不受 schema 演进带来的列序差异影响。
        let mut label_cols: Vec<(&str, &StringArray)> = schema
            .fields()
            .iter()
            .enumerate()
            .filter_map(|(i, f)| {
                let name = f.name().as_str();
                if name == "_timestamp" || name == "value" {
                    return None;
                }
                if !matches!(f.data_type(), DataType::Utf8) {
                    return None;
                }
                b.column(i)
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .map(|arr| (name, arr))
            })
            .collect();
        label_cols.sort_by_key(|(name, _)| *name);
        let matcher_cols = MatcherColumns::resolve(matchers, &label_cols);
        let metric_name_col = logical_metric.and_then(|_| {
            label_cols
                .iter()
                .position(|(name, _)| *name == METRIC_NAME_FIELD)
        });

        for row in 0..b.num_rows() {
            if val_arr.is_null(row) || ts_arr.is_null(row) {
                continue;
            }
            let ts = ts_arr.value(row);
            if ts <= start_us || ts > end_us {
                continue;
            }
            if let Some(expected) = logical_metric {
                let Some(index) = metric_name_col else {
                    continue;
                };
                let metric_names = label_cols[index].1;
                if metric_names.is_null(row) || metric_names.value(row) != expected {
                    continue;
                }
            }
            if !matcher_cols.matches(&label_cols, row) {
                continue;
            }

            key_buf.clear();
            for (name, arr) in &label_cols {
                if logical_metric.is_some() && is_metric_identity_storage_field(name) {
                    continue;
                }
                if arr.is_null(row) {
                    continue;
                }
                key_buf.push_str(name);
                key_buf.push('\u{1}');
                key_buf.push_str(arr.value(row));
                key_buf.push('\u{2}');
            }
            let idx = match by_key.get(key_buf.as_str()) {
                Some(&i) => i,
                None => {
                    let mut labels = LabelSet::new();
                    for (name, arr) in &label_cols {
                        if logical_metric.is_some() && is_metric_identity_storage_field(name) {
                            continue;
                        }
                        if !arr.is_null(row) {
                            labels.insert((*name).to_string(), arr.value(row).to_string());
                        }
                    }
                    series.push(Series {
                        labels,
                        samples: Vec::new(),
                    });
                    by_key.insert(key_buf.clone(), series.len() - 1);
                    series.len() - 1
                }
            };
            total += 1;
            if total > max_samples {
                return Err(Error::invalid(format!(
                    "promql selector matched more than {max_samples} samples; \
                     narrow the time window or add label matchers"
                )));
            }
            series[idx].samples.push((ts, val_arr.value(row)));
        }
    }
    for s in &mut series {
        s.samples.sort_by_key(|&(t, _)| t);
        // Multiple batches can contain the same series/timestamp (for example
        // retrying an ingest). Keep the most recently materialized value so a
        // duplicate point cannot create a zero-duration or artificial reset.
        let mut deduplicated: Vec<(i64, f64)> = Vec::with_capacity(s.samples.len());
        for sample in s.samples.drain(..) {
            if let Some(last) = deduplicated.last_mut()
                && last.0 == sample.0
            {
                *last = sample;
                continue;
            }
            deduplicated.push(sample);
        }
        s.samples = deduplicated;
    }
    // 稳定排序：series 间按 label 字典序
    series.sort_by(|a, b| a.labels.cmp(&b.labels));
    Ok(series)
}

/// matchers 与 label 列的预解析绑定：每个 matcher 解析一次列下标（缺列＝label
/// 视为空串），行循环里直接读列值判断，避免先构建 LabelSet 再匹配。
struct MatcherColumns<'m> {
    plain: Vec<(Option<usize>, &'m Matcher)>,
    or_groups: Vec<Vec<(Option<usize>, &'m Matcher)>>,
}

impl<'m> MatcherColumns<'m> {
    fn resolve(matchers: &'m Matchers, label_cols: &[(&str, &StringArray)]) -> Self {
        let col_of = |m: &Matcher| label_cols.iter().position(|(name, _)| *name == m.name);
        Self {
            plain: matchers.matchers.iter().map(|m| (col_of(m), m)).collect(),
            or_groups: matchers
                .or_matchers
                .iter()
                .map(|g| g.iter().map(|m| (col_of(m), m)).collect())
                .collect(),
        }
    }

    /// PromQL 选择器匹配语义：plain matchers 全部命中，且（若有 or 组）任一
    /// or 组整组命中。直接读列值，不构建 LabelSet。
    fn matches(&self, label_cols: &[(&str, &StringArray)], row: usize) -> bool {
        for (col, m) in &self.plain {
            if !match_row(*col, m, label_cols, row) {
                return false;
            }
        }
        for group in &self.or_groups {
            if group
                .iter()
                .all(|(col, m)| match_row(*col, m, label_cols, row))
            {
                return true;
            }
        }
        self.or_groups.is_empty() || !self.plain.is_empty()
    }
}

fn match_row(
    col: Option<usize>,
    m: &Matcher,
    label_cols: &[(&str, &StringArray)],
    row: usize,
) -> bool {
    if m.name == "__name__" {
        return true;
    }
    let actual = col
        .map(|i| {
            let arr = label_cols[i].1;
            if arr.is_null(row) { "" } else { arr.value(row) }
        })
        .unwrap_or("");
    matcher_matches_value(m, actual)
}

pub(super) fn matcher_matches_value(m: &Matcher, actual: &str) -> bool {
    match &m.op {
        MatchOp::Equal => actual == m.value,
        MatchOp::NotEqual => actual != m.value,
        MatchOp::Re(r) => r.is_match(actual),
        MatchOp::NotRe(r) => !r.is_match(actual),
    }
}
