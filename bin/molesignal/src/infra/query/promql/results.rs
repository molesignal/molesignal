// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;

/// 把 instant 结果转为 [`QueryResult`] —— 列布局 `[ts, value, ...labels]`。
pub(super) fn instant_to_query_result(v: InstantVector, started: Instant) -> QueryResult {
    let mut all_labels: BTreeMap<String, ()> = BTreeMap::new();
    for (ls, _) in &v.items {
        for k in ls.keys() {
            all_labels.insert(k.clone(), ());
        }
    }
    let mut columns = vec!["value".to_string()];
    columns.extend(all_labels.keys().cloned());

    let rows: Vec<Vec<serde_json::Value>> = v
        .items
        .into_iter()
        .map(|(ls, val)| {
            let mut row = vec![serde_json::json!(val)];
            for k in all_labels.keys() {
                row.push(
                    ls.get(k)
                        .map(|s| serde_json::Value::String(s.clone()))
                        .unwrap_or(serde_json::Value::Null),
                );
            }
            row
        })
        .collect();
    let scanned = rows.len() as u64;
    QueryResult {
        columns,
        rows,
        scanned_rows: scanned,
        took_ms: started.elapsed().as_millis() as u64,
        federation: None,
    }
}

/// 在全局行数上限内为每条 series 公平分配点数，并在每条 series 内均匀取样。
///
/// 相比直接保留最后 N 行，这会尽量保留每条 series 的首尾时间点和完整时间跨度。
/// 当 series 数本身超过上限时，只能按 label 稳定顺序保留前 N 条 series 的最新点。
fn limit_range_points(points: Vec<RangePoint>, limit: Option<usize>) -> Vec<RangePoint> {
    let Some(limit) = limit.filter(|limit| *limit > 0) else {
        return points;
    };
    if points.len() <= limit {
        return points;
    }

    let mut grouped: BTreeMap<LabelSet, Vec<RangePoint>> = BTreeMap::new();
    for point in points {
        grouped.entry(point.labels.clone()).or_default().push(point);
    }
    let mut series = grouped.into_values().collect::<Vec<_>>();
    for points in &mut series {
        points.sort_by_key(|point| point.ts_us);
    }

    let series_count = series.len();
    let base = limit / series_count;
    let remainder = limit % series_count;
    let mut budgets = series
        .iter()
        .enumerate()
        .map(|(index, points)| (base + usize::from(index < remainder)).min(points.len()))
        .collect::<Vec<_>>();

    // Short series may not consume their equal share. Redistribute that
    // capacity deterministically among series that still have points.
    let mut unused = limit.saturating_sub(budgets.iter().sum());
    while unused > 0 {
        let active = budgets
            .iter()
            .enumerate()
            .filter_map(|(index, budget)| (*budget < series[index].len()).then_some(index))
            .collect::<Vec<_>>();
        if active.is_empty() {
            break;
        }
        let share = (unused / active.len()).max(1);
        for index in active {
            let added = share.min(series[index].len() - budgets[index]).min(unused);
            budgets[index] += added;
            unused -= added;
            if unused == 0 {
                break;
            }
        }
    }

    let mut limited = Vec::with_capacity(limit);
    for (points, budget) in series.into_iter().zip(budgets) {
        match budget {
            0 => {}
            1 => {
                if let Some(point) = points.into_iter().next_back() {
                    limited.push(point);
                }
            }
            budget if budget >= points.len() => limited.extend(points),
            budget => {
                let last = points.len() - 1;
                let mut points = points.into_iter().map(Some).collect::<Vec<_>>();
                for slot in 0..budget {
                    let index = slot * last / (budget - 1);
                    if let Some(point) = points[index].take() {
                        limited.push(point);
                    }
                }
            }
        }
    }
    limited
}

/// 把 range 结果转为 [`QueryResult`] —— 列布局 `[_timestamp, value, ...labels]`。
pub(super) fn range_to_query_result(
    mut v: RangeVector,
    started: Instant,
    limit: Option<usize>,
) -> QueryResult {
    v.points = limit_range_points(v.points, limit);
    v.points
        .sort_by(|a, b| a.ts_us.cmp(&b.ts_us).then_with(|| a.labels.cmp(&b.labels)));

    let mut all_labels: BTreeMap<String, ()> = BTreeMap::new();
    for point in &v.points {
        for k in point.labels.keys() {
            all_labels.insert(k.clone(), ());
        }
    }

    let mut columns = vec!["_timestamp".to_string(), "value".to_string()];
    columns.extend(all_labels.keys().cloned());

    let rows: Vec<Vec<serde_json::Value>> = v
        .points
        .into_iter()
        .map(|point| {
            let mut row = vec![
                serde_json::json!(point.ts_us),
                serde_json::json!(point.value),
            ];
            for k in all_labels.keys() {
                row.push(
                    point
                        .labels
                        .get(k)
                        .map(|s| serde_json::Value::String(s.clone()))
                        .unwrap_or(serde_json::Value::Null),
                );
            }
            row
        })
        .collect();
    let scanned = rows.len() as u64;
    QueryResult {
        columns,
        rows,
        scanned_rows: scanned,
        took_ms: started.elapsed().as_millis() as u64,
        federation: None,
    }
}
