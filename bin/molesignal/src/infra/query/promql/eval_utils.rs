// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;

pub(super) fn validate_binary_modifier(binary: &BinaryExpr) -> Result<()> {
    let Some(modifier) = &binary.modifier else {
        return Ok(());
    };

    // `default` fill-in（未匹配填充）尚未实现，其余 on/ignoring/group_left/right
    // 与集合运算（and/or/unless）均已支持。
    if modifier.fill_values.lhs.is_some() || modifier.fill_values.rhs.is_some() {
        return Err(Error::invalid(
            "promql binary `default` fill values not yet supported",
        ));
    }

    Ok(())
}

/// 把选择器上的 `@`（pin 时刻）与 `offset`（前/后移）解析成生效求值时刻（micros）。
pub(super) fn effective_eval_time(vs: &VectorSelector, at_us: i64, req: &QueryRequest) -> i64 {
    resolve_modifier_time(vs.at.as_ref(), vs.offset.as_ref(), at_us, req)
}

/// 子查询窗口的右端时刻（受其自身 `@` / `offset` 影响）。
pub(super) fn subquery_eval_end(sq: &SubqueryExpr, at_us: i64, req: &QueryRequest) -> i64 {
    resolve_modifier_time(sq.at.as_ref(), sq.offset.as_ref(), at_us, req)
}

fn resolve_modifier_time(
    at: Option<&AtModifier>,
    offset: Option<&Offset>,
    at_us: i64,
    req: &QueryRequest,
) -> i64 {
    pinned_time(at, req).unwrap_or(at_us) - offset_micros(offset)
}

/// `@` 固定的求值时刻（micros）；没有 `@` 时为 `None`。
pub(super) fn pinned_time(at: Option<&AtModifier>, req: &QueryRequest) -> Option<i64> {
    at.map(|at| match at {
        AtModifier::Start => req.time_range.start.0,
        AtModifier::End => req.time_range.end.0,
        AtModifier::At(t) => system_time_to_micros(*t),
    })
}

/// `offset` 把求值时刻往前移的微秒数；负的 offset 往后移，是负数。
pub(super) fn offset_micros(offset: Option<&Offset>) -> i64 {
    match offset {
        Some(Offset::Pos(d)) => d.as_micros() as i64,
        Some(Offset::Neg(d)) => -(d.as_micros() as i64),
        None => 0,
    }
}

fn system_time_to_micros(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_micros() as i64,
        Err(e) => -(e.duration().as_micros() as i64),
    }
}

pub(super) fn expr_is_scalar(expr: &Expr) -> bool {
    match expr {
        Expr::NumberLiteral(_) => true,
        Expr::Paren(paren) => expr_is_scalar(paren.expr.as_ref()),
        Expr::Unary(unary) => expr_is_scalar(unary.expr.as_ref()),
        Expr::Binary(binary) => {
            expr_is_scalar(binary.lhs.as_ref()) && expr_is_scalar(binary.rhs.as_ref())
        }
        Expr::Call(call) => call_is_scalar(call),
        _ => false,
    }
}

pub(super) fn call_is_scalar(call: &Call) -> bool {
    let name = call.func.name;

    matches!(name, "pi" | "time" | "scalar")
        || (is_sample_math_function(name)
            && call
                .args
                .args
                .iter()
                .all(|arg| expr_is_scalar(arg.as_ref())))
}

pub(super) fn scalar_value(input: InstantVector, context: &str) -> Result<f64> {
    let mut items = input.items.into_iter();

    let Some((labels, value)) = items.next() else {
        return Err(Error::invalid(format!(
            "{context} must evaluate to a scalar"
        )));
    };

    if !labels.is_empty() || items.next().is_some() {
        return Err(Error::invalid(format!(
            "{context} must evaluate to a scalar"
        )));
    }

    Ok(value)
}

pub(super) fn negate_instant_vector(mut input: InstantVector) -> InstantVector {
    for (_, value) in &mut input.items {
        *value = -*value;
    }

    input
}

pub(super) fn negate_range_vector(mut input: RangeVector) -> RangeVector {
    for point in &mut input.points {
        point.value = -point.value;
    }

    input
}

/// 在 `steps` 的每个步点 `t` 回调它的窗口右端 `end`（见 [`Steps::window_end`]），与前置窗口
/// `(end - range_us, end]` 内的样本切片（`samples` 须按 ts 升序）。
///
/// 两指针实现：lo/hi 只单调前移，整条 series 共 O(samples + steps)。range 求值
/// 一律走这里按 step 出点，而不是对窗口内每个原始样本各出一个点——后者输出规模
/// 随采样密度线性、重扫成本随密度平方膨胀。
pub(super) fn each_step_window<F>(samples: &[(i64, f64)], steps: Steps, range_us: i64, mut visit: F)
where
    F: FnMut(i64, i64, &[(i64, f64)]),
{
    each_step_window_indices(samples, steps, range_us, |t, end, lo, hi| {
        visit(t, end, &samples[lo..hi])
    });
}

/// Index-based variant used when per-sample sidecar metadata must stay aligned
/// with the selected value window.
pub(super) fn each_step_window_indices<F>(
    samples: &[(i64, f64)],
    steps: Steps,
    range_us: i64,
    mut visit: F,
) where
    F: FnMut(i64, i64, usize, usize),
{
    let step_us = steps.step_us.max(1);
    let range_us = range_us.max(1);
    let (mut lo, mut hi) = (0usize, 0usize);
    let mut t = steps.start_us;

    while t <= steps.end_us {
        // 窗口右端随步点前移（`@` 时不动），两个指针也只前移。
        let end = steps.window_end(t);
        let window_start = end.saturating_sub(range_us);

        while lo < samples.len() && samples[lo].0 <= window_start {
            lo += 1;
        }

        if hi < lo {
            hi = lo;
        }

        while hi < samples.len() && samples[hi].0 <= end {
            hi += 1;
        }

        visit(t, end, lo, hi);

        match t.checked_add(step_us) {
            Some(next) => t = next,
            None => break,
        }
    }
}
