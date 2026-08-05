// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! DataFusion AggregateUDFs（Search 差异化）。
//!
//! `approx_topk(col) -> TEXT`：列值的近似 Top-K 高频项（K=10），返回 JSON
//! `[{"value":..,"count":..}, ...]`（count 降序）。用有界计数表（space-saving 风格：
//! 超 `MAX_TRACKED` 时驱逐当前最小项，把名额让给新项并继承最小计数）控内存，
//! 适合「top error messages / top services」这类高基数列。支持部分聚合（state/merge），
//! 跨分区可并行。非 Utf8 列自动 cast 到 Utf8。

use std::{collections::HashMap, sync::Arc};

use arrow::{
    array::{Array, ArrayRef, StringArray},
    datatypes::{DataType, Field, FieldRef},
};
use datafusion::{
    error::{DataFusionError, Result as DfResult},
    logical_expr::{
        Accumulator, AggregateUDF, AggregateUDFImpl, Signature, TypeSignature, Volatility,
        function::{AccumulatorArgs, StateFieldsArgs},
    },
    physical_expr::expressions::Literal,
    scalar::ScalarValue,
};

/// 默认返回的高频项数（`approx_topk(col)` 不带 k 时）。
const DEFAULT_K: usize = 10;
/// 计数表上限（有界内存）；超出按 space-saving 驱逐最小项。
const MAX_TRACKED: usize = 1024;

/// 构造 `approx_topk` AggregateUDF。
pub fn approx_topk_udf() -> AggregateUDF {
    AggregateUDF::from(ApproxTopK::new())
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct ApproxTopK {
    signature: Signature,
}

impl ApproxTopK {
    fn new() -> Self {
        Self {
            // `approx_topk(col)` 或 `approx_topk(col, k)`：1 或 2 个任意类型参数
            // （col accumulator 内 cast 到 Utf8；k 为整型字面量，见 accumulator）。
            signature: Signature::one_of(
                vec![TypeSignature::Any(1), TypeSignature::Any(2)],
                Volatility::Immutable,
            ),
        }
    }
}

/// 从聚合参数里取第 2 个（k）字面量；缺省 / 非正整数 → [`DEFAULT_K`]。
fn k_from_args(acc_args: &AccumulatorArgs) -> usize {
    acc_args
        .exprs
        .get(1)
        .and_then(|e| e.downcast_ref::<Literal>())
        .and_then(|lit| match lit.value() {
            ScalarValue::Int64(Some(v)) if *v > 0 => Some(*v as usize),
            ScalarValue::UInt64(Some(v)) if *v > 0 => Some(*v as usize),
            _ => None,
        })
        .unwrap_or(DEFAULT_K)
}

impl AggregateUDFImpl for ApproxTopK {
    fn name(&self) -> &str {
        "approx_topk"
    }
    fn signature(&self) -> &Signature {
        &self.signature
    }
    fn return_type(&self, _arg_types: &[DataType]) -> DfResult<DataType> {
        Ok(DataType::Utf8)
    }
    fn accumulator(&self, acc_args: AccumulatorArgs) -> DfResult<Box<dyn Accumulator>> {
        Ok(Box::new(ApproxTopKAccumulator::with_k(k_from_args(
            &acc_args,
        ))))
    }
    fn state_fields(&self, _args: StateFieldsArgs) -> DfResult<Vec<FieldRef>> {
        // 单个 Utf8 字段：序列化后的计数表 JSON。
        Ok(vec![Arc::new(Field::new("counts", DataType::Utf8, true))])
    }
}

#[derive(Debug)]
struct ApproxTopKAccumulator {
    counts: HashMap<String, i64>,
    k: usize,
}

impl Default for ApproxTopKAccumulator {
    fn default() -> Self {
        Self {
            counts: HashMap::new(),
            k: DEFAULT_K,
        }
    }
}

impl ApproxTopKAccumulator {
    fn with_k(k: usize) -> Self {
        Self {
            counts: HashMap::new(),
            k: k.max(1),
        }
    }

    /// 累加 `key` 计数 `by`；表满时按 space-saving 驱逐最小项。
    fn bump(&mut self, key: String, by: i64) {
        if let Some(c) = self.counts.get_mut(&key) {
            *c += by;
            return;
        }
        if self.counts.len() >= MAX_TRACKED
            && let Some((min_k, min_c)) = self
                .counts
                .iter()
                .min_by_key(|(_, c)| **c)
                .map(|(k, c)| (k.clone(), *c))
        {
            self.counts.remove(&min_k);
            self.counts.insert(key, min_c + by);
            return;
        }
        self.counts.insert(key, by);
    }

    fn topk(&self) -> Vec<(String, i64)> {
        let mut v: Vec<(String, i64)> = self.counts.iter().map(|(k, c)| (k.clone(), *c)).collect();
        // count 降序，平手按 value 升序（结果稳定）。
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v.truncate(self.k);
        v
    }
}

fn as_utf8(arr: &ArrayRef) -> DfResult<ArrayRef> {
    if arr.data_type() == &DataType::Utf8 {
        return Ok(arr.clone());
    }
    arrow::compute::cast(arr, &DataType::Utf8)
        .map_err(|e| DataFusionError::Execution(format!("approx_topk cast to utf8: {e}")))
}

impl Accumulator for ApproxTopKAccumulator {
    fn update_batch(&mut self, values: &[ArrayRef]) -> DfResult<()> {
        let Some(arr) = values.first() else {
            return Ok(());
        };
        let casted = as_utf8(arr)?;
        let sa = casted
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| {
                DataFusionError::Execution("approx_topk expects a castable-to-utf8 column".into())
            })?;
        for i in 0..sa.len() {
            if !sa.is_null(i) {
                self.bump(sa.value(i).to_string(), 1);
            }
        }
        Ok(())
    }

    fn evaluate(&mut self) -> DfResult<ScalarValue> {
        let arr: Vec<serde_json::Value> = self
            .topk()
            .into_iter()
            .map(|(value, count)| serde_json::json!({ "value": value, "count": count }))
            .collect();
        Ok(ScalarValue::Utf8(Some(
            serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into()),
        )))
    }

    fn size(&self) -> usize {
        std::mem::size_of_val(self) + self.counts.keys().map(|k| k.len() + 16).sum::<usize>()
    }

    fn state(&mut self) -> DfResult<Vec<ScalarValue>> {
        // 整张计数表序列化为 JSON 作部分聚合状态。
        let json = serde_json::to_string(&self.counts).unwrap_or_else(|_| "{}".into());
        Ok(vec![ScalarValue::Utf8(Some(json))])
    }

    fn merge_batch(&mut self, states: &[ArrayRef]) -> DfResult<()> {
        let Some(arr) = states.first() else {
            return Ok(());
        };
        let sa = arr.as_any().downcast_ref::<StringArray>().ok_or_else(|| {
            DataFusionError::Execution("approx_topk merge expects utf8 state".into())
        })?;
        for i in 0..sa.len() {
            if sa.is_null(i) {
                continue;
            }
            if let Ok(map) = serde_json::from_str::<HashMap<String, i64>>(sa.value(i)) {
                for (k, c) in map {
                    self.bump(k, c);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_returns_topk_desc() {
        let mut a = ApproxTopKAccumulator::default();
        let arr: ArrayRef = Arc::new(StringArray::from(vec!["a", "a", "a", "b", "b", "c"]));
        a.update_batch(&[arr]).unwrap();
        let json = match a.evaluate().unwrap() {
            ScalarValue::Utf8(Some(s)) => s,
            _ => panic!("expected utf8"),
        };
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed[0]["value"], "a");
        assert_eq!(parsed[0]["count"], 3);
        assert_eq!(parsed[1]["value"], "b");
        assert_eq!(parsed[1]["count"], 2);
    }

    #[test]
    fn merge_combines_partial_states() {
        let mut a = ApproxTopKAccumulator::default();
        a.bump("x".into(), 2);
        let mut b = ApproxTopKAccumulator::default();
        b.bump("x".into(), 5);
        b.bump("y".into(), 1);
        let state = match b.state().unwrap().pop().unwrap() {
            ScalarValue::Utf8(Some(s)) => s,
            _ => panic!("expected utf8 state"),
        };
        let arr: ArrayRef = Arc::new(StringArray::from(vec![state]));
        a.merge_batch(&[arr]).unwrap();
        assert_eq!(*a.counts.get("x").unwrap(), 7);
        assert_eq!(*a.counts.get("y").unwrap(), 1);
    }

    #[test]
    fn null_values_are_ignored() {
        let mut a = ApproxTopKAccumulator::default();
        let arr: ArrayRef = Arc::new(StringArray::from(vec![Some("a"), None, Some("a")]));
        a.update_batch(&[arr]).unwrap();
        assert_eq!(*a.counts.get("a").unwrap(), 2);
        assert_eq!(a.counts.len(), 1);
    }

    /// 端到端：UDAF 真正经 DataFusion planner 规划 + 执行（验证 signature / 注册正确）。
    #[tokio::test]
    async fn udaf_plans_and_runs_in_datafusion() {
        use arrow::{datatypes::Schema, record_batch::RecordBatch};
        use datafusion::{datasource::MemTable, prelude::SessionContext};

        let ctx = SessionContext::new();
        ctx.register_udaf(approx_topk_udf());
        let schema = Arc::new(Schema::new(vec![Field::new("svc", DataType::Utf8, true)]));
        let arr: ArrayRef = Arc::new(StringArray::from(vec![
            "api", "api", "api", "web", "web", "db",
        ]));
        let batch = RecordBatch::try_new(schema.clone(), vec![arr]).unwrap();
        let mem = MemTable::try_new(schema, vec![vec![batch]]).unwrap();
        ctx.register_table("t", Arc::new(mem)).unwrap();

        let out = ctx
            .sql("SELECT approx_topk(svc) AS tk FROM t")
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();
        let s = out[0]
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0)
            .to_string();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed[0]["value"], "api");
        assert_eq!(parsed[0]["count"], 3);
    }

    /// 端到端：`approx_topk(col, k)` 的字面量 k 生效（这里 k=2 → 只返 2 项）。
    #[tokio::test]
    async fn udaf_respects_k_parameter() {
        use arrow::{datatypes::Schema, record_batch::RecordBatch};
        use datafusion::{datasource::MemTable, prelude::SessionContext};

        let ctx = SessionContext::new();
        ctx.register_udaf(approx_topk_udf());
        let schema = Arc::new(Schema::new(vec![Field::new("svc", DataType::Utf8, true)]));
        let arr: ArrayRef = Arc::new(StringArray::from(vec!["a", "a", "b", "b", "c", "c", "d"]));
        let batch = RecordBatch::try_new(schema.clone(), vec![arr]).unwrap();
        let mem = MemTable::try_new(schema, vec![vec![batch]]).unwrap();
        ctx.register_table("t", Arc::new(mem)).unwrap();

        let out = ctx
            .sql("SELECT approx_topk(svc, 2) AS tk FROM t")
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();
        let s = out[0]
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0)
            .to_string();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed.len(), 2, "k=2 should cap output to 2 items");
    }
}
