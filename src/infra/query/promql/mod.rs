// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PromQL 引擎。
//!
//! 支持的函数子集（不在此名单的返 `Error::Invalid("promql function not yet supported: <name>")`）：
//! - range：`rate`/`irate`/`increase`、`delta`/`idelta`/`deriv`/`predict_linear`/`resets`/
//!   `changes`/`holt_winters`、全部 `*_over_time`（avg/min/max/sum/count/quantile/stddev/
//!   stdvar/last/present/mad）
//! - 聚合（含 `by/without`）：`sum|avg|min|max|count|stddev|stdvar|quantile|group|count_values|
//!   topk|bottomk|limitk|limit_ratio`
//! - `histogram_quantile(q, sum by(le)(rate(metric_bucket[range])))`：bucket 分位数；
//!   `histogram_fraction(lower, upper, v)`：classic bucket 落在 `[lower, upper]` 的观测占比
//! - label：`label_replace` / `label_join`；排序：`sort|sort_desc|sort_by_label|sort_by_label_desc`
//! - 时间：`time|timestamp|minute|hour|day_of_week|day_of_month|day_of_year|days_in_month|month|year`
//! - 类型/缺失：`vector|scalar|absent|absent_over_time`
//! - 数学：`abs|ceil|floor|round|exp|ln|log2|log10|sqrt|sgn|clamp*|sin|cos|...|pi|deg|rad`
//! - 二元：算术/比较（scalar↔vector、1:1）、集合运算 `and|or|unless`、向量匹配
//!   `on|ignoring` + `group_left|group_right`、一元负号
//! - 修饰/结构：选择器 `@` 与 `offset`、子查询 `inner[range:step]`
//!
//! 未实现（classic bucket 模型下 N/A）：native-histogram 系列 `histogram_count|histogram_sum|
//! histogram_avg|histogram_stddev|histogram_stdvar`、二元 `default` 填充。
//!
//! Instant query：在 `req.time_range.end` 处求值。
//! Range query：在 `[req.time_range.start, req.time_range.end]` 按 step 步进，
//! 每个步点对其前置窗口求值，结果合成 `matrix`（输出 `(ts_us, value)` 序列每个 label 集）。
//! step 由跨度与 `limit` 推导（[`range_step_us`]），每条 series 输出 ≤ [`MAX_RANGE_STEPS`]
//! 个点，与原始采样密度无关；单 selector 物化样本数受 [`MAX_MATRIX_SAMPLES`] 约束。
//!
//! 列布局约定：metrics stream schema 必须含 `value Float64`；其余字段通常视为 labels。
//! 由 `IngestService::ingest` 写入时，每行的 `_timestamp` 由 RawEvent.timestamp 决定，
//! `value` / labels 由 RawEvent.fields 提供。Prometheus Exemplar 旁路行不含 `value`，
//! 因此 sample evaluator 会跳过它们，`query_exemplars` 走独立读取路径。系统指标的
//! `metric_name` / `metric_kind` 是容器存储元数据，不暴露成逻辑指标的 labels。

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use arrow::{
    array::{Array, Float64Array, RecordBatch, StringArray, TimestampMicrosecondArray},
    datatypes::DataType,
};
use async_trait::async_trait;
use object_store::ObjectStore;
use promql_parser::{
    label::{MatchOp, Matcher, Matchers},
    parser::{
        self, AggregateExpr, AtModifier, BinaryExpr, Call, Expr, FunctionArgs, LabelModifier,
        MatrixSelector, Offset, SubqueryExpr, UnaryExpr, VectorMatchCardinality, VectorSelector,
        token::{
            T_ADD, T_DIV, T_EQLC, T_GTE, T_GTR, T_LAND, T_LOR, T_LSS, T_LTE, T_LUNLESS, T_MOD,
            T_MUL, T_NEQ, T_POW, T_SUB, TokenType,
        },
    },
};
use regex::{Captures, Regex};

use crate::{
    domain::{
        query::{PromqlEngine, QueryRequest, QueryResult},
        storage::ParquetFileMetaRepository,
        stream::{StreamRepository, StreamType},
    },
    infra::storage::parquet::reader::{ParquetReader, ReadOptions},
    shared::{Error, Result, time::TimeRange},
};

mod aggregate;
mod args;
mod binary;
pub mod capabilities;
mod datetime;
mod eval_utils;
mod exemplars;
mod functions;
mod incremental;
mod labels;
mod math;
mod metric_source;
mod range_funcs;
mod results;
mod series;
#[cfg(test)]
mod tests;
mod types;

use aggregate::*;
use args::*;
use binary::*;
use datetime::*;
use eval_utils::*;
use functions::*;
// Exposed for criterion benches (`benches/*`); not part of the stable API.
#[doc(hidden)]
pub use functions::{apply_histogram_quantile, apply_rate_like};
use incremental::StreamingAgg;
use labels::*;
use math::*;
use range_funcs::*;
use results::{instant_to_query_result, range_to_query_result};
use series::*;
pub use types::{InstantVector, LabelSet, Series};
use types::{RangePoint, RangeVector};

/// Prometheus staleness lookback：instant 取样与裸 selector range 步进时向前
/// 回看的窗口。
const STALENESS_LOOKBACK_US: i64 = 5 * 60 * 1_000_000;

/// range 查询每条 series 的输出点数上限。请求没有显式 step 参数，按
/// 「时间跨度 / 目标点数」推导 step（目标点数取 `limit`，封顶本值）；输出点数
/// 与原始采样密度无关——这是 range 路径的 step 降采样。
const MAX_RANGE_STEPS: i64 = 1_000;

/// 单个 selector 一次扫描可物化的样本数上限（约 160 MB 的 `(ts, value)` 对）。
/// 超过即报错提示收窄窗口 / 加 matcher，避免无界内存与超时。
const MAX_MATRIX_SAMPLES: usize = 10_000_000;

/// range 查询的步长（micros）：`span / min(limit, MAX_RANGE_STEPS)`，至少 1µs。
fn range_step_us(req: &QueryRequest) -> i64 {
    let span_us = req
        .time_range
        .end
        .0
        .saturating_sub(req.time_range.start.0)
        .max(1);
    let target = req
        .limit
        .map(|l| l as i64)
        .unwrap_or(MAX_RANGE_STEPS)
        .clamp(1, MAX_RANGE_STEPS);
    (span_us / target).max(1)
}

/// 递归收集表达式里所有 vector/matrix selector 的 metric 名（去重由调用方的 `BTreeSet` 负责）。
/// 覆盖求值器支持的全部 `Expr` 变体；字面量（Number/String）无 selector，命中 `_` 分支跳过——
/// 任何其它变体本就不被求值器支持、会在 eval 阶段报错，故不会漏过能返回数据的查询。
fn collect_metric_names(expr: &Expr, out: &mut std::collections::BTreeSet<String>) {
    match expr {
        Expr::VectorSelector(vs) => {
            if let Some(name) = &vs.name {
                out.insert(name.clone());
            }
        }
        Expr::MatrixSelector(ms) => {
            if let Some(name) = &ms.vs.name {
                out.insert(name.clone());
            }
        }
        Expr::Paren(p) => collect_metric_names(&p.expr, out),
        Expr::Unary(u) => collect_metric_names(&u.expr, out),
        Expr::Binary(b) => {
            collect_metric_names(&b.lhs, out);
            collect_metric_names(&b.rhs, out);
        }
        Expr::Aggregate(a) => {
            collect_metric_names(&a.expr, out);
            if let Some(param) = &a.param {
                collect_metric_names(param, out);
            }
        }
        Expr::Subquery(sq) => collect_metric_names(&sq.expr, out),
        Expr::Call(c) => {
            for arg in &c.args.args {
                collect_metric_names(arg, out);
            }
        }
        _ => {}
    }
}

pub(crate) fn referenced_metric_names(
    statement: &str,
) -> Result<std::collections::BTreeSet<String>> {
    let expression = parser::parse(statement)
        .map_err(|error| Error::invalid(format!("promql parse: {error}")))?;
    let mut names = std::collections::BTreeSet::new();
    collect_metric_names(&expression, &mut names);
    Ok(names)
}

fn collect_derived_label_dependencies(
    expression: &Expr,
    output: &mut Vec<(String, Vec<String>)>,
) -> Result<()> {
    match expression {
        Expr::Paren(paren) => collect_derived_label_dependencies(&paren.expr, output)?,
        Expr::Unary(unary) => collect_derived_label_dependencies(&unary.expr, output)?,
        Expr::Binary(binary) => {
            collect_derived_label_dependencies(&binary.lhs, output)?;
            collect_derived_label_dependencies(&binary.rhs, output)?;
        }
        Expr::Aggregate(aggregate) => {
            collect_derived_label_dependencies(&aggregate.expr, output)?;
            if let Some(parameter) = &aggregate.param {
                collect_derived_label_dependencies(parameter, output)?;
            }
        }
        Expr::Subquery(subquery) => {
            collect_derived_label_dependencies(&subquery.expr, output)?;
        }
        Expr::Call(call) => {
            match call.func.name {
                "label_replace" => {
                    let args = label_replace_args(&call.args)?;
                    output.push((args.dst_label, vec![args.src_label]));
                }
                "label_join" => {
                    let args = label_join_args(&call.args)?;
                    output.push((args.dst_label, args.src_labels));
                }
                _ => {}
            }
            for argument in &call.args.args {
                collect_derived_label_dependencies(argument, output)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn derived_label_dependencies(statement: &str) -> Result<Vec<(String, Vec<String>)>> {
    let expression = parser::parse(statement)
        .map_err(|error| Error::invalid(format!("promql parse: {error}")))?;
    let mut dependencies = Vec::new();
    collect_derived_label_dependencies(&expression, &mut dependencies)?;
    Ok(dependencies)
}

pub struct PromQLEngine {
    files: Arc<dyn ParquetFileMetaRepository>,
    object_store: Arc<dyn ObjectStore>,
    /// range 窗口聚合增量缓存；`None` = 未装配（range 路径行为与现状一致）。
    streaming: Option<StreamingAgg>,
    /// 可选 stream 目录；装配后，命中的 metric stream 若标记为不可查询则拒绝。
    /// `None` = 不校验（保持历史单测行为）。
    streams: Option<Arc<dyn StreamRepository>>,
}

impl PromQLEngine {
    pub fn new(
        files: Arc<dyn ParquetFileMetaRepository>,
        object_store: Arc<dyn ObjectStore>,
    ) -> Self {
        Self {
            files,
            object_store,
            streaming: None,
            streams: None,
        }
    }

    /// 注入 StreamRepository：metric stream 标记为不可查询（`settings.queryable == false`，
    /// 仅作 ingest 入口 / pipeline 源）时拒绝查询。缺省（不调用）则不做该校验。
    pub fn with_streams(mut self, streams: Arc<dyn StreamRepository>) -> Self {
        self.streams = Some(streams);
        self
    }

    /// 校验表达式引用到的所有 metric stream 均可查询：存在但 `queryable == false` → 拒绝；
    /// 不存在 → 放行（保持 PromQL 空集语义）。未注入 streams 时直接放行。
    async fn ensure_metrics_queryable(&self, req: &QueryRequest, expr: &Expr) -> Result<()> {
        let Some(streams) = &self.streams else {
            return Ok(());
        };
        let mut names = std::collections::BTreeSet::new();
        collect_metric_names(expr, &mut names);
        for name in names {
            let source = self.resolve_metric_source(&req.org_id, &name).await?;
            let Some(stream_id) = source.stream_id else {
                continue;
            };
            if !streams.get_settings(&stream_id).await?.queryable {
                return Err(Error::forbidden(format!("stream is not queryable: {name}")));
            }
        }
        Ok(())
    }

    /// 装配 range 窗口聚合增量缓存（实时仪表盘刷新只重算活跃区）。`safe_lookback`
    /// 是安全回看窗口：仅封存窗口右端早于 `now - safe_lookback` 的桶，近段全划活跃以
    /// 规避晚到数据。不调用本方法时 range 路径零行为变化。
    pub fn with_streaming_cache(
        mut self,
        cache: Arc<crate::infra::caching::StreamingAggCache>,
        safe_lookback: Duration,
    ) -> Self {
        self.streaming = Some(StreamingAgg {
            cache,
            safe_lookback_us: safe_lookback.as_micros() as i64,
        });
        self
    }
}

#[async_trait]
impl PromqlEngine for PromQLEngine {
    #[tracing::instrument(
        name = "query.promql",
        skip_all,
        fields(otel.kind = "internal", molesignal.query.engine = "promql")
    )]
    async fn execute(&self, req: QueryRequest) -> Result<QueryResult> {
        let started = Instant::now();
        let expr = parser::parse(&req.statement)
            .map_err(|e| Error::invalid(format!("promql parse: {e}")))?;

        // queryable 闸门：先于缓存/求值，校验查询引用到的所有 metric stream。存在但被标记
        // 不可查询 → 拒绝；不存在则放行（PromQL「选择不存在的指标得空集」语义，不报错）。
        // 放在最前面，确保 streaming agg cache 全命中时也不会绕过该校验。
        self.ensure_metrics_queryable(&req, &expr).await?;

        let should_range = req.limit.is_some_and(|limit| limit > 1)
            && req.time_range.end.0 > req.time_range.start.0;
        if should_range && let Some(vec) = self.eval_range(&expr, &req).await? {
            // `range_step_us` bounds evaluation work per series. The result
            // adapter then applies the request-wide row limit with
            // series-aware temporal sampling so cardinality cannot multiply
            // the response past `limit`.
            return Ok(range_to_query_result(vec, started, req.limit));
        }

        // Instant evaluation at time_range.end. Alert-style scalar probes pass
        // limit=1 and keep this path so existing threshold evaluation stays
        // unchanged.
        let at_us = req.time_range.end.0;
        let vec = self.eval(&expr, &req, at_us).await?;
        let result = instant_to_query_result(vec, started);
        Ok(result)
    }

    async fn query_exemplars(
        &self,
        req: QueryRequest,
    ) -> Result<crate::domain::metrics::PrometheusExemplarQueryResult> {
        self.query_exemplars_inner(req).await
    }
}

// =====================================================================
//  Evaluator
// =====================================================================

impl PromQLEngine {
    async fn eval_range(&self, expr: &Expr, req: &QueryRequest) -> Result<Option<RangeVector>> {
        match expr {
            Expr::Paren(p) => Box::pin(self.eval_range(&p.expr, req)).await,
            Expr::Unary(unary) => self.eval_range_unary(unary, req).await,
            Expr::Binary(binary) => self.eval_range_binary(binary, req).await,
            Expr::Aggregate(agg) => self.eval_range_aggregate(agg, req).await,
            Expr::Call(call) => self.eval_range_call(call, req).await,
            Expr::VectorSelector(vs) => self.eval_range_vector_selector(vs, req).await.map(Some),
            _ => Ok(None),
        }
    }

    async fn eval_range_unary(
        &self,
        unary: &UnaryExpr,
        req: &QueryRequest,
    ) -> Result<Option<RangeVector>> {
        let Some(inner) = Box::pin(self.eval_range(&unary.expr, req)).await? else {
            return Ok(None);
        };
        Ok(Some(negate_range_vector(inner)))
    }

    async fn eval_range_binary(
        &self,
        binary: &BinaryExpr,
        req: &QueryRequest,
    ) -> Result<Option<RangeVector>> {
        validate_binary_modifier(binary)?;

        let lhs_is_scalar = expr_is_scalar(binary.lhs.as_ref());
        let rhs_is_scalar = expr_is_scalar(binary.rhs.as_ref());
        match (lhs_is_scalar, rhs_is_scalar) {
            (true, true) => Ok(None),
            (true, false) => {
                let lhs =
                    Box::pin(self.eval(binary.lhs.as_ref(), req, req.time_range.end.0)).await?;
                let lhs = scalar_value(lhs, "binary lhs")?;
                let Some(rhs) = Box::pin(self.eval_range(binary.rhs.as_ref(), req)).await? else {
                    return Ok(None);
                };
                apply_binary_range_scalar(binary, lhs, rhs, true).map(Some)
            }
            (false, true) => {
                let Some(lhs) = Box::pin(self.eval_range(binary.lhs.as_ref(), req)).await? else {
                    return Ok(None);
                };
                let rhs =
                    Box::pin(self.eval(binary.rhs.as_ref(), req, req.time_range.end.0)).await?;
                let rhs = scalar_value(rhs, "binary rhs")?;
                apply_binary_range_scalar(binary, rhs, lhs, false).map(Some)
            }
            (false, false) => {
                let Some(lhs) = Box::pin(self.eval_range(binary.lhs.as_ref(), req)).await? else {
                    return Ok(None);
                };
                let Some(rhs) = Box::pin(self.eval_range(binary.rhs.as_ref(), req)).await? else {
                    return Ok(None);
                };
                apply_binary_range(binary, lhs, rhs).map(Some)
            }
        }
    }

    async fn eval_range_aggregate(
        &self,
        agg: &AggregateExpr,
        req: &QueryRequest,
    ) -> Result<Option<RangeVector>> {
        let Some(inner) = Box::pin(self.eval_range(&agg.expr, req)).await? else {
            return Ok(None);
        };
        let op_name = agg.op.to_string();
        let modifier = agg.modifier.clone();
        match op_name.as_str() {
            "topk" | "bottomk" => {
                let k = self
                    .eval_aggregate_param(agg, req, req.time_range.end.0)
                    .await?;
                return apply_topk_bottomk_range(op_name.as_str(), k, inner, modifier.as_ref())
                    .map(Some);
            }
            "limitk" => {
                let k = self
                    .eval_aggregate_param(agg, req, req.time_range.end.0)
                    .await?;
                return apply_limitk_range(k, inner, modifier.as_ref()).map(Some);
            }
            "limit_ratio" => {
                let ratio = self
                    .eval_aggregate_param(agg, req, req.time_range.end.0)
                    .await?;
                return apply_limit_ratio_range(ratio, inner).map(Some);
            }
            _ => {}
        }
        let param = match op_name.as_str() {
            "quantile" => Some(AggregateParam::Scalar(
                self.eval_aggregate_param(agg, req, req.time_range.end.0)
                    .await?,
            )),
            "count_values" => Some(AggregateParam::LabelName(aggregate_string_param(agg)?)),
            _ => None,
        };

        let mut grouped: HashMap<(i64, LabelSet), Vec<f64>> = HashMap::new();
        for point in inner.points {
            let key = project_labels(&point.labels, modifier.as_ref());
            grouped
                .entry((point.ts_us, key))
                .or_default()
                .push(point.value);
        }

        let mut points = Vec::with_capacity(grouped.len());
        for ((ts_us, labels), values) in grouped {
            for (labels, value) in
                apply_regular_aggregate(op_name.as_str(), labels, values, param.as_ref())?
            {
                points.push(RangePoint {
                    ts_us,
                    labels,
                    value,
                });
            }
        }

        Ok(Some(RangeVector { points }))
    }

    async fn eval_range_call(
        &self,
        call: &Call,
        req: &QueryRequest,
    ) -> Result<Option<RangeVector>> {
        let func_name = call.func.name;
        match func_name {
            name if capabilities::is_function_category(
                name,
                capabilities::FunctionCategory::Rate,
            ) =>
            {
                let arg = expr_arg(&call.args, 0, func_name)?;
                if let Some(ms) = self.cacheable_matrix(arg) {
                    let range = ms.range;
                    let range_secs = range.as_secs_f64().max(1.0);
                    let rv = self
                        .eval_windowed_cached(&ms.vs, range, func_name, req, move |_t, win| {
                            rate_window_value(func_name, win, range_secs)
                        })
                        .await?;
                    return Ok(Some(rv));
                }
                let Some((series, range)) = self.eval_matrix_input_range(arg, req).await? else {
                    return Ok(None);
                };
                Ok(Some(apply_rate_like_range(
                    func_name,
                    series,
                    range,
                    req.time_range.start.0,
                    req.time_range.end.0,
                    range_step_us(req),
                )))
            }
            name if is_over_time_function(name) => {
                let (quantile, matrix_idx) = over_time_args(name, &call.args)?;
                let arg = expr_arg(&call.args, matrix_idx, name)?;
                if let Some(ms) = self.cacheable_matrix(arg) {
                    let func_key = format!("{name}|q={quantile:?}");
                    let rv = self
                        .eval_windowed_cached(&ms.vs, ms.range, &func_key, req, move |_t, win| {
                            apply_over_time_value(name, quantile, win)
                        })
                        .await?;
                    return Ok(Some(rv));
                }
                let Some((series, range)) = self.eval_matrix_input_range(arg, req).await? else {
                    return Ok(None);
                };
                Ok(Some(apply_over_time_range(
                    name,
                    quantile,
                    series,
                    range,
                    req.time_range.start.0,
                    req.time_range.end.0,
                    range_step_us(req),
                )))
            }
            name if is_range_vector_function(name) => {
                let params = range_vector_args(name, &call.args)?;
                let arg = expr_arg(&call.args, 0, name)?;
                if let Some(ms) = self.cacheable_matrix(arg) {
                    let func_key = format!(
                        "{name}|pt={}|sf={}|tf={}",
                        params.predict_t, params.hw_sf, params.hw_tf
                    );
                    let rv = self
                        .eval_windowed_cached(&ms.vs, ms.range, &func_key, req, move |t, win| {
                            range_vector_value(name, win, t, params)
                        })
                        .await?;
                    return Ok(Some(rv));
                }
                let Some((series, range)) = self.eval_matrix_input_range(arg, req).await? else {
                    return Ok(None);
                };
                Ok(Some(apply_range_vector_func_range(
                    name,
                    series,
                    range,
                    req.time_range.start.0,
                    req.time_range.end.0,
                    range_step_us(req),
                    params,
                )))
            }
            "label_replace" => {
                let args = label_replace_args(&call.args)?;
                let Some(inner) = Box::pin(self.eval_range(&args.expr, req)).await? else {
                    return Ok(None);
                };
                Ok(Some(apply_label_replace_range(inner, &args)))
            }
            "label_join" => {
                let args = label_join_args(&call.args)?;
                let Some(inner) = Box::pin(self.eval_range(&args.expr, req)).await? else {
                    return Ok(None);
                };
                Ok(Some(apply_label_join_range(inner, &args)))
            }
            name if is_sample_math_function(name) => {
                let input_expr = expr_arg(&call.args, 0, name)?;
                let Some(inner) = Box::pin(self.eval_range(input_expr, req)).await? else {
                    return Ok(None);
                };
                let params = self
                    .sample_math_params(name, &call.args, req, req.time_range.end.0)
                    .await?;
                apply_sample_math_range(name, inner, params).map(Some)
            }
            name if is_datetime_function(name) => {
                // 无参 datetime() = vector(time())：range 路径无 series 可映射，
                // 退回 instant 求值（与 time()/scalar 等同行为）。
                if call.args.is_empty() {
                    return Ok(None);
                }
                let input_expr = expr_arg(&call.args, 0, name)?;
                let Some(inner) = Box::pin(self.eval_range(input_expr, req)).await? else {
                    return Ok(None);
                };
                Ok(Some(apply_datetime_range(name, inner)))
            }
            "timestamp" => {
                let input_expr = expr_arg(&call.args, 0, "timestamp")?;
                let Some(inner) = Box::pin(self.eval_range(input_expr, req)).await? else {
                    return Ok(None);
                };
                Ok(Some(apply_timestamp_range(inner)))
            }
            _ => Ok(None),
        }
    }

    async fn eval_range_vector_selector(
        &self,
        vs: &VectorSelector,
        req: &QueryRequest,
    ) -> Result<RangeVector> {
        // 多拉一个 staleness lookback，保证最早的步点也能向前找到样本。
        let series = self
            .load_matrix_for_query_range(
                vs,
                Duration::from_micros(STALENESS_LOOKBACK_US as u64),
                req,
            )
            .await?;
        Ok(samples_to_range_vector(
            series,
            req.time_range.start.0,
            req.time_range.end.0,
            range_step_us(req),
        ))
    }

    /// instant evaluator：在 `at_us` 这个时间点对 expr 求值。
    async fn eval(&self, expr: &Expr, req: &QueryRequest, at_us: i64) -> Result<InstantVector> {
        match expr {
            Expr::NumberLiteral(n) => Ok(InstantVector {
                items: vec![(LabelSet::new(), n.val)],
            }),
            Expr::Paren(p) => Box::pin(self.eval(&p.expr, req, at_us)).await,
            Expr::Unary(unary) => self.eval_unary(unary, req, at_us).await,
            Expr::Binary(binary) => self.eval_binary(binary, req, at_us).await,
            Expr::Aggregate(agg) => self.eval_aggregate(agg, req, at_us).await,
            Expr::Call(call) => self.eval_call(call, req, at_us).await,
            Expr::VectorSelector(vs) => self.eval_instant_vector(vs, req, at_us).await,
            Expr::MatrixSelector(_) => Err(Error::invalid(
                "MatrixSelector cannot appear standalone; wrap with rate()/increase()/...",
            )),
            other => Err(Error::invalid(format!(
                "promql expression not supported: {other:?}"
            ))),
        }
    }

    async fn eval_unary(
        &self,
        unary: &UnaryExpr,
        req: &QueryRequest,
        at_us: i64,
    ) -> Result<InstantVector> {
        let inner = Box::pin(self.eval(&unary.expr, req, at_us)).await?;
        Ok(negate_instant_vector(inner))
    }

    async fn eval_binary(
        &self,
        binary: &BinaryExpr,
        req: &QueryRequest,
        at_us: i64,
    ) -> Result<InstantVector> {
        validate_binary_modifier(binary)?;

        let lhs_is_scalar = expr_is_scalar(binary.lhs.as_ref());
        let rhs_is_scalar = expr_is_scalar(binary.rhs.as_ref());
        let lhs = Box::pin(self.eval(binary.lhs.as_ref(), req, at_us)).await?;
        let rhs = Box::pin(self.eval(binary.rhs.as_ref(), req, at_us)).await?;
        apply_binary_instant(binary, lhs, rhs, lhs_is_scalar, rhs_is_scalar)
    }

    /// 拉取整个查询时间范围，以及 range 函数计算所需的前置窗口。
    async fn load_matrix_for_query_range(
        &self,
        vs: &VectorSelector,
        matrix_range: Duration,
        req: &QueryRequest,
    ) -> Result<Vec<Series>> {
        let span_us = req
            .time_range
            .end
            .0
            .saturating_sub(req.time_range.start.0)
            .max(1);
        let matrix_us = (matrix_range.as_micros() as i64).max(1);
        let total_us = span_us.saturating_add(matrix_us).max(1);
        self.load_matrix(
            vs,
            Duration::from_micros(total_us as u64),
            req.time_range.end.0,
            req,
        )
        .await
    }

    /// instant vector selector：取 (at_us - 5min, at_us] 内每个 series 的最新一个样本。
    async fn eval_instant_vector(
        &self,
        vs: &VectorSelector,
        req: &QueryRequest,
        at_us: i64,
    ) -> Result<InstantVector> {
        let series = self
            .load_matrix(
                vs,
                Duration::from_micros(STALENESS_LOOKBACK_US as u64),
                at_us,
                req,
            )
            .await?;
        let items: Vec<_> = series
            .into_iter()
            .filter_map(|s| s.samples.last().map(|&(_, v)| (s.labels, v)))
            .collect();
        Ok(InstantVector { items })
    }

    async fn eval_aggregate(
        &self,
        agg: &AggregateExpr,
        req: &QueryRequest,
        at_us: i64,
    ) -> Result<InstantVector> {
        let inner = Box::pin(self.eval(&agg.expr, req, at_us)).await?;
        let op_name = agg.op.to_string();
        let modifier = agg.modifier.clone();
        match op_name.as_str() {
            "topk" | "bottomk" => {
                let k = self.eval_aggregate_param(agg, req, at_us).await?;
                return apply_topk_bottomk(op_name.as_str(), k, inner, modifier.as_ref());
            }
            "limitk" => {
                let k = self.eval_aggregate_param(agg, req, at_us).await?;
                return apply_limitk(k, inner, modifier.as_ref());
            }
            "limit_ratio" => {
                let ratio = self.eval_aggregate_param(agg, req, at_us).await?;
                return apply_limit_ratio(ratio, inner);
            }
            _ => {}
        }
        let param = match op_name.as_str() {
            "quantile" => Some(AggregateParam::Scalar(
                self.eval_aggregate_param(agg, req, at_us).await?,
            )),
            "count_values" => Some(AggregateParam::LabelName(aggregate_string_param(agg)?)),
            _ => None,
        };

        let grouped = group_by(&inner, modifier.as_ref());
        let mut out = Vec::with_capacity(grouped.len());
        for (key, values) in grouped {
            out.extend(apply_regular_aggregate(
                op_name.as_str(),
                key,
                values,
                param.as_ref(),
            )?);
        }
        Ok(InstantVector { items: out })
    }

    async fn eval_aggregate_param(
        &self,
        agg: &AggregateExpr,
        req: &QueryRequest,
        at_us: i64,
    ) -> Result<f64> {
        let param = agg.param.as_ref().ok_or_else(|| {
            Error::invalid(format!("promql aggregate requires parameter: {}", agg.op))
        })?;
        let value = Box::pin(self.eval(param, req, at_us)).await?;
        scalar_value(value, "aggregate parameter")
    }

    async fn eval_call(
        &self,
        call: &Call,
        req: &QueryRequest,
        at_us: i64,
    ) -> Result<InstantVector> {
        let func_name = call.func.name;
        match func_name {
            name if capabilities::is_function_category(
                name,
                capabilities::FunctionCategory::Rate,
            ) =>
            {
                let arg = expr_arg(&call.args, 0, func_name)?;
                let (series, range) = self.eval_matrix_input(arg, at_us, req).await?;
                Ok(apply_rate_like(func_name, series, range))
            }
            name if is_over_time_function(name) => {
                let (quantile, matrix_idx) = over_time_args(name, &call.args)?;
                let arg = expr_arg(&call.args, matrix_idx, name)?;
                let (series, _) = self.eval_matrix_input(arg, at_us, req).await?;
                Ok(apply_over_time(name, quantile, series))
            }
            name if is_range_vector_function(name) => {
                let params = range_vector_args(name, &call.args)?;
                let arg = expr_arg(&call.args, 0, name)?;
                let (series, _) = self.eval_matrix_input(arg, at_us, req).await?;
                Ok(apply_range_vector_func(name, series, at_us, params))
            }
            "histogram_quantile" => {
                let (q, inner_expr) = histogram_quantile_args(&call.args)?;
                let inner = Box::pin(self.eval(&inner_expr, req, at_us)).await?;
                Ok(apply_histogram_quantile(q, inner))
            }
            "histogram_fraction" => {
                ensure_arg_count("histogram_fraction", &call.args, 3)?;
                let lower = self
                    .eval_scalar_function_arg(&call.args, 0, req, at_us, "histogram_fraction lower")
                    .await?;
                let upper = self
                    .eval_scalar_function_arg(&call.args, 1, req, at_us, "histogram_fraction upper")
                    .await?;
                let inner_expr = expr_arg(&call.args, 2, "histogram_fraction")?;
                let inner = Box::pin(self.eval(inner_expr, req, at_us)).await?;
                Ok(apply_histogram_fraction(lower, upper, inner))
            }
            "label_replace" => {
                let args = label_replace_args(&call.args)?;
                let inner = Box::pin(self.eval(&args.expr, req, at_us)).await?;
                Ok(apply_label_replace(inner, &args))
            }
            "label_join" => {
                let args = label_join_args(&call.args)?;
                let inner = Box::pin(self.eval(&args.expr, req, at_us)).await?;
                Ok(apply_label_join(inner, &args))
            }
            "sort" | "sort_desc" => {
                ensure_arg_count(func_name, &call.args, 1)?;
                let input_expr = expr_arg(&call.args, 0, func_name)?;
                let inner = Box::pin(self.eval(input_expr, req, at_us)).await?;
                Ok(sort_instant_vector(inner, func_name == "sort_desc"))
            }
            "sort_by_label" | "sort_by_label_desc" => {
                let labels = sort_by_label_labels(&call.args)?;
                let input_expr = expr_arg(&call.args, 0, func_name)?;
                let inner = Box::pin(self.eval(input_expr, req, at_us)).await?;
                Ok(sort_instant_vector_by_label(
                    inner,
                    &labels,
                    func_name == "sort_by_label_desc",
                ))
            }
            "absent" => {
                ensure_arg_count("absent", &call.args, 1)?;
                let input_expr = expr_arg(&call.args, 0, "absent")?;
                let inner = Box::pin(self.eval(input_expr, req, at_us)).await?;
                Ok(absent_vector(input_expr, inner))
            }
            "absent_over_time" => {
                ensure_arg_count("absent_over_time", &call.args, 1)?;
                let ms = matrix_arg(&call.args)?;
                let series = self.load_matrix(&ms.vs, ms.range, at_us, req).await?;
                let present = series.iter().any(|s| !s.samples.is_empty());
                Ok(absent_over_time_vector(&ms, present))
            }
            "time" => {
                ensure_arg_count("time", &call.args, 0)?;
                Ok(time_instant_vector(at_us))
            }
            name if is_datetime_function(name) => {
                ensure_arg_range(name, &call.args, 0, 1)?;
                let inner = if call.args.is_empty() {
                    time_instant_vector(at_us)
                } else {
                    let input_expr = expr_arg(&call.args, 0, name)?;
                    Box::pin(self.eval(input_expr, req, at_us)).await?
                };
                Ok(apply_datetime(name, inner))
            }
            "timestamp" => {
                ensure_arg_count("timestamp", &call.args, 1)?;
                let input_expr = expr_arg(&call.args, 0, "timestamp")?;
                let inner = Box::pin(self.eval(input_expr, req, at_us)).await?;
                Ok(apply_timestamp(inner, at_us))
            }
            "vector" => {
                ensure_arg_count("vector", &call.args, 1)?;
                let input_expr = expr_arg(&call.args, 0, "vector")?;
                let value = Box::pin(self.eval(input_expr, req, at_us)).await?;
                Ok(vector_from_scalar(scalar_value(value, "vector argument")?))
            }
            "scalar" => {
                ensure_arg_count("scalar", &call.args, 1)?;
                let input_expr = expr_arg(&call.args, 0, "scalar")?;
                let inner = Box::pin(self.eval(input_expr, req, at_us)).await?;
                Ok(scalar_from_vector(inner))
            }
            "pi" => {
                ensure_arg_count("pi", &call.args, 0)?;
                Ok(InstantVector {
                    items: vec![(LabelSet::new(), std::f64::consts::PI)],
                })
            }
            name if is_sample_math_function(name) => {
                let input_expr = expr_arg(&call.args, 0, name)?;
                let inner = Box::pin(self.eval(input_expr, req, at_us)).await?;
                let params = self
                    .sample_math_params(name, &call.args, req, at_us)
                    .await?;
                apply_sample_math(name, inner, params)
            }
            other => Err(Error::invalid(format!(
                "promql function not yet supported: {other}"
            ))),
        }
    }

    async fn sample_math_params(
        &self,
        name: &str,
        args: &FunctionArgs,
        req: &QueryRequest,
        at_us: i64,
    ) -> Result<SampleMathParams> {
        match name {
            "round" => {
                ensure_arg_range("round", args, 1, 2)?;
                let nearest = if args.len() == 2 {
                    self.eval_scalar_function_arg(args, 1, req, at_us, "round to_nearest")
                        .await?
                } else {
                    1.0
                };
                Ok(SampleMathParams::Round { nearest })
            }
            "clamp" => {
                ensure_arg_count("clamp", args, 3)?;
                let min = self
                    .eval_scalar_function_arg(args, 1, req, at_us, "clamp min")
                    .await?;
                let max = self
                    .eval_scalar_function_arg(args, 2, req, at_us, "clamp max")
                    .await?;
                Ok(SampleMathParams::Clamp { min, max })
            }
            "clamp_min" => {
                ensure_arg_count("clamp_min", args, 2)?;
                let min = self
                    .eval_scalar_function_arg(args, 1, req, at_us, "clamp_min min")
                    .await?;
                Ok(SampleMathParams::ClampMin { min })
            }
            "clamp_max" => {
                ensure_arg_count("clamp_max", args, 2)?;
                let max = self
                    .eval_scalar_function_arg(args, 1, req, at_us, "clamp_max max")
                    .await?;
                Ok(SampleMathParams::ClampMax { max })
            }
            _ => {
                ensure_arg_count(name, args, 1)?;
                Ok(SampleMathParams::None)
            }
        }
    }

    async fn eval_scalar_function_arg(
        &self,
        args: &FunctionArgs,
        index: usize,
        req: &QueryRequest,
        at_us: i64,
        context: &str,
    ) -> Result<f64> {
        let arg = expr_arg(args, index, context)?;
        let value = Box::pin(self.eval(arg, req, at_us)).await?;
        scalar_value(value, context)
    }

    /// 从 `(at_us - range, at_us]` 拉一个 matrix（每个 series 的样本序列）。
    /// 实现：按 ParquetFileMeta 裁剪时间窗，Parquet 只投影 timestamp/value/label 列并做
    /// row-group pruning，再在内存按 matchers 过滤与按 labels 分组。
    async fn load_matrix(
        &self,
        vs: &VectorSelector,
        range: Duration,
        at_us: i64,
        req: &QueryRequest,
    ) -> Result<Vec<Series>> {
        let metric = vs
            .name
            .clone()
            .ok_or_else(|| Error::invalid("VectorSelector missing metric name"))?;
        let source = self.resolve_metric_source(&req.org_id, &metric).await?;
        // 选择器上的 `@` / `offset` 修饰：先把求值时刻平移到生效时刻。
        let at_us = effective_eval_time(vs, at_us, req);
        let range_us = range.as_micros() as i64;
        let start_us = at_us - range_us;

        let parquet_file_metas = self
            .metric_files(
                &req.org_id,
                &source.stream,
                TimeRange::new(
                    crate::shared::time::TimestampMicros(start_us),
                    crate::shared::time::TimestampMicros(at_us),
                ),
            )
            .await?;
        if parquet_file_metas.is_empty() {
            return Ok(Vec::new());
        }
        let reader = ParquetReader::new(self.object_store.clone());
        let mut batches: Vec<RecordBatch> = Vec::new();
        let projection = source
            .sample_columns
            .as_ref()
            .map(|columns| columns.iter().map(String::as_str).collect::<Vec<_>>());
        for fm in &parquet_file_metas {
            // 时间窗口下推：按 `_timestamp` row-group 统计裁剪，边界文件只解码
            // 与窗口相交的 row group；ParquetFileMeta 里的权威 size 避免每文件
            // 再做一次 object_store HEAD。
            let mut options = ReadOptions::new()
                .with_time_range(start_us, at_us)
                .with_known_size(fm.size_bytes);
            if let Some(columns) = projection.as_deref() {
                options = options.with_columns(columns);
            }
            let bs = reader
                .read_from_store(self.object_store.clone(), &fm.object_key, options)
                .await?;
            batches.extend(bs);
        }
        if batches.is_empty() {
            return Ok(Vec::new());
        }

        batches_to_series(
            &batches,
            &vs.matchers,
            source.logical_metric.as_deref(),
            start_us,
            at_us,
            MAX_MATRIX_SAMPLES,
        )
    }

    /// 求值 range 函数的「矩阵参数」（`metric[range]`）。同时支持原生 matrix selector
    /// 与子查询 `inner[range:step]`，统一返回 `(series, range)`。
    async fn eval_matrix_input(
        &self,
        arg: &Expr,
        at_us: i64,
        req: &QueryRequest,
    ) -> Result<(Vec<Series>, Duration)> {
        match arg {
            Expr::MatrixSelector(ms) => {
                let series = self.load_matrix(&ms.vs, ms.range, at_us, req).await?;
                Ok((series, ms.range))
            }
            Expr::Subquery(sq) => {
                let series = self.eval_subquery(sq, at_us, req).await?;
                Ok((series, sq.range))
            }
            other => Err(Error::invalid(format!(
                "expected a range-vector argument (`metric[5m]` or subquery `expr[5m:1m]`), got {other:?}"
            ))),
        }
    }

    /// range 查询路径：matrix selector 一次拉整段；子查询返回 `None` 让外层回退到
    /// instant（避免外层步进 × 子查询步进的双重开销）。
    async fn eval_matrix_input_range(
        &self,
        arg: &Expr,
        req: &QueryRequest,
    ) -> Result<Option<(Vec<Series>, Duration)>> {
        match arg {
            Expr::MatrixSelector(ms) => {
                let series = self
                    .load_matrix_for_query_range(&ms.vs, ms.range, req)
                    .await?;
                Ok(Some((series, ms.range)))
            }
            Expr::Subquery(_) => Ok(None),
            other => Err(Error::invalid(format!(
                "expected a range-vector argument, got {other:?}"
            ))),
        }
    }

    /// 子查询 `inner[range:step]`：在 `(end-range, end]` 内按 `step` 逐点对 `inner` 做
    /// instant 求值，按 label 集汇成 series 喂给外层 range 函数。`step` 缺省 60s，
    /// 点数上限 11000 防止解析过细导致的扫描爆炸。
    async fn eval_subquery(
        &self,
        sq: &SubqueryExpr,
        at_us: i64,
        req: &QueryRequest,
    ) -> Result<Vec<Series>> {
        const DEFAULT_STEP_US: i64 = 60_000_000;
        const MAX_STEPS: i64 = 11_000;
        let step_us = sq
            .step
            .map(|d| (d.as_micros() as i64).max(1))
            .unwrap_or(DEFAULT_STEP_US);
        let range_us = (sq.range.as_micros() as i64).max(1);
        if range_us / step_us > MAX_STEPS {
            return Err(Error::invalid(
                "subquery resolution too fine (exceeds 11000 steps)",
            ));
        }
        let end_us = subquery_eval_end(sq, at_us, req);
        let start_us = end_us - range_us;

        let mut series_map: BTreeMap<LabelSet, Vec<(i64, f64)>> = BTreeMap::new();
        let mut t = start_us;
        while t <= end_us {
            let iv = Box::pin(self.eval(&sq.expr, req, t)).await?;
            for (labels, value) in iv.items {
                series_map.entry(labels).or_default().push((t, value));
            }
            t += step_us;
        }
        Ok(series_map
            .into_iter()
            .map(|(labels, samples)| Series { labels, samples })
            .collect())
    }
}
