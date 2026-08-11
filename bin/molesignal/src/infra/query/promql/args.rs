// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;

pub(super) fn matrix_arg(args: &FunctionArgs) -> Result<MatrixSelector> {
    let first = args
        .first()
        .ok_or_else(|| Error::invalid("function expects a matrix selector argument"))?;
    match *first {
        Expr::MatrixSelector(ref m) => Ok(m.clone()),
        _ => Err(Error::invalid(
            "function expects a matrix selector like `metric[5m]`",
        )),
    }
}

pub(super) fn expr_arg<'a>(
    args: &'a FunctionArgs,
    index: usize,
    context: &str,
) -> Result<&'a Expr> {
    args.args
        .get(index)
        .map(|arg| arg.as_ref())
        .ok_or_else(|| Error::invalid(format!("{context} missing arg{index}")))
}

pub(super) fn ensure_arg_count(name: &str, args: &FunctionArgs, expected: usize) -> Result<()> {
    if args.len() == expected {
        return Ok(());
    }
    Err(Error::invalid(format!(
        "{name} expects {expected} argument(s), got {}",
        args.len()
    )))
}

pub(super) fn ensure_arg_range(
    name: &str,
    args: &FunctionArgs,
    min: usize,
    max: usize,
) -> Result<()> {
    if (min..=max).contains(&args.len()) {
        return Ok(());
    }
    Err(Error::invalid(format!(
        "{name} expects {min}..={max} argument(s), got {}",
        args.len()
    )))
}

pub(super) fn is_over_time_function(name: &str) -> bool {
    super::capabilities::is_function_category(name, super::capabilities::FunctionCategory::OverTime)
}

pub(super) fn is_sample_math_function(name: &str) -> bool {
    super::capabilities::is_function_category(name, super::capabilities::FunctionCategory::Math)
}

/// 解析 `*_over_time` 参数，返回 `(quantile, matrix_arg_index)`。矩阵参数本身
/// （matrix selector 或 subquery）由调用方经 `eval_matrix_input` 求值。
pub(super) fn over_time_args(name: &str, args: &FunctionArgs) -> Result<(Option<f64>, usize)> {
    if name != "quantile_over_time" {
        if args.is_empty() {
            return Err(Error::invalid(format!(
                "{name} expects a range-vector argument"
            )));
        }
        return Ok((None, 0));
    }
    if args.len() < 2 {
        return Err(Error::invalid(
            "quantile_over_time expects (quantile, matrix)",
        ));
    }
    Ok((Some(number_arg(args, 0, "quantile_over_time quantile")?), 1))
}

pub(super) fn histogram_quantile_args(args: &FunctionArgs) -> Result<(f64, Expr)> {
    if args.len() < 2 {
        return Err(Error::invalid(
            "histogram_quantile expects (quantile, vector)",
        ));
    }
    let arg0 = args
        .args
        .first()
        .ok_or_else(|| Error::invalid("histogram_quantile missing arg0"))?;
    let arg1 = args
        .args
        .get(1)
        .ok_or_else(|| Error::invalid("histogram_quantile missing arg1"))?;
    let q = match **arg0 {
        Expr::NumberLiteral(ref n) => n.val,
        _ => {
            return Err(Error::invalid(
                "histogram_quantile arg0 must be a scalar quantile",
            ));
        }
    };
    Ok((q, *arg1.clone()))
}

#[derive(Debug, Clone)]
pub(super) struct LabelReplaceArgs {
    pub(super) expr: Expr,
    pub(super) dst_label: String,
    pub(super) replacement: String,
    pub(super) src_label: String,
    pub(super) regex: Regex,
}

pub(super) fn label_replace_args(args: &FunctionArgs) -> Result<LabelReplaceArgs> {
    if args.len() < 5 {
        return Err(Error::invalid(
            "label_replace expects (vector, dst_label, replacement, src_label, regex)",
        ));
    }
    let expr = args
        .args
        .first()
        .ok_or_else(|| Error::invalid("label_replace missing arg0"))?;
    let dst_label = string_arg(args, 1, "label_replace dst_label")?;
    let replacement = string_arg(args, 2, "label_replace replacement")?;
    let src_label = string_arg(args, 3, "label_replace src_label")?;
    let regex = string_arg(args, 4, "label_replace regex")?;
    let regex = Regex::new(&regex)
        .map_err(|e| Error::invalid(format!("label_replace invalid regex: {e}")))?;
    Ok(LabelReplaceArgs {
        expr: *expr.clone(),
        dst_label,
        replacement,
        src_label,
        regex,
    })
}

#[derive(Debug, Clone)]
pub(super) struct LabelJoinArgs {
    pub(super) expr: Expr,
    pub(super) dst_label: String,
    pub(super) separator: String,
    pub(super) src_labels: Vec<String>,
}

pub(super) fn label_join_args(args: &FunctionArgs) -> Result<LabelJoinArgs> {
    if args.len() < 4 {
        return Err(Error::invalid(
            "label_join expects (vector, dst_label, separator, src_label, ...)",
        ));
    }
    let expr = args
        .args
        .first()
        .ok_or_else(|| Error::invalid("label_join missing arg0"))?;
    let dst_label = string_arg(args, 1, "label_join dst_label")?;
    let separator = string_arg(args, 2, "label_join separator")?;
    let src_labels = (3..args.len())
        .map(|index| string_arg(args, index, "label_join src_label"))
        .collect::<Result<Vec<_>>>()?;
    Ok(LabelJoinArgs {
        expr: *expr.clone(),
        dst_label,
        separator,
        src_labels,
    })
}

pub(super) fn number_arg(args: &FunctionArgs, index: usize, context: &str) -> Result<f64> {
    let arg = args
        .args
        .get(index)
        .ok_or_else(|| Error::invalid(format!("{context} missing")))?;
    match **arg {
        Expr::NumberLiteral(ref n) => Ok(n.val),
        _ => Err(Error::invalid(format!(
            "{context} must be a scalar literal"
        ))),
    }
}

/// 解析 range-vector 派生函数（delta/deriv/predict_linear/holt_winters/…）的标量参数
/// （predict_linear 的 t、holt_winters 的 sf/tf）。矩阵参数恒为 arg0，由调用方求值。
pub(super) fn range_vector_args(name: &str, args: &FunctionArgs) -> Result<RangeFuncParams> {
    let mut params = RangeFuncParams::default();
    match name {
        "predict_linear" => {
            ensure_arg_count("predict_linear", args, 2)?;
            params.predict_t = number_arg(args, 1, "predict_linear t")?;
        }
        "holt_winters" | "double_exponential_smoothing" => {
            ensure_arg_count(name, args, 3)?;
            params.hw_sf = number_arg(args, 1, "holt_winters smoothing factor")?;
            params.hw_tf = number_arg(args, 2, "holt_winters trend factor")?;
            if !(0.0..=1.0).contains(&params.hw_sf) || !(0.0..=1.0).contains(&params.hw_tf) {
                return Err(Error::invalid(
                    "holt_winters smoothing/trend factors must be in [0,1]",
                ));
            }
        }
        _ => ensure_arg_count(name, args, 1)?,
    }
    Ok(params)
}

pub(super) fn string_arg(args: &FunctionArgs, index: usize, context: &str) -> Result<String> {
    let arg = args
        .args
        .get(index)
        .ok_or_else(|| Error::invalid(format!("{context} missing")))?;
    match **arg {
        Expr::StringLiteral(ref s) => Ok(s.val.clone()),
        _ => Err(Error::invalid(format!("{context} must be a string"))),
    }
}

pub(super) fn sort_by_label_labels(args: &FunctionArgs) -> Result<Vec<String>> {
    if args.len() < 2 {
        return Err(Error::invalid("sort_by_label expects (vector, label, ...)"));
    }
    (1..args.len())
        .map(|index| string_arg(args, index, "sort_by_label label"))
        .collect()
}

pub(super) fn aggregate_string_param(agg: &AggregateExpr) -> Result<String> {
    let param = agg.param.as_ref().ok_or_else(|| {
        Error::invalid(format!("promql aggregate requires parameter: {}", agg.op))
    })?;
    match **param {
        Expr::StringLiteral(ref s) => Ok(s.val.clone()),
        _ => Err(Error::invalid(format!(
            "promql aggregate parameter must be a string: {}",
            agg.op
        ))),
    }
}
