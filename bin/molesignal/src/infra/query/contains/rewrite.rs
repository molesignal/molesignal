// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The analyzer rule that turns substring `LIKE`s into [`contains_text`](super) calls.
//!
//! Rewritten: `col [NOT] LIKE '%x%'` and `col [NOT] ILIKE '%x%'` where `x` has no wildcard and no
//! backslash (and, for `ILIKE`, only ASCII), with no `ESCAPE`. An `OR` of two such calls on the same
//! column with the same case flag becomes one call with both needles, so a chain of them is one
//! scan. Patterns with a fixed prefix (`'x%'`) stay `LIKE`: file and page statistics prune with
//! them. The rule runs after type coercion.

use std::sync::Arc;

use datafusion::{
    common::{
        config::ConfigOptions,
        tree_node::{Transformed, TreeNode},
    },
    error::Result,
    logical_expr::{
        Expr, LogicalPlan, Operator,
        expr::{BinaryExpr, Like, ScalarFunction},
    },
    optimizer::AnalyzerRule,
    scalar::ScalarValue,
};

use super::{NAME, contains_text_udf, scalar_text};

#[derive(Debug, Default)]
pub(crate) struct ContainsLiteral;

impl AnalyzerRule for ContainsLiteral {
    fn name(&self) -> &str {
        "contains_literal"
    }

    fn analyze(&self, plan: LogicalPlan, _config: &ConfigOptions) -> Result<LogicalPlan> {
        plan.transform_up_with_subqueries(|node| {
            node.map_expressions(|expr| expr.transform_up(rewrite))
        })
        .map(|transformed| transformed.data)
    }
}

fn rewrite(expr: Expr) -> Result<Transformed<Expr>> {
    Ok(match expr {
        Expr::Like(like) => match call_for(&like) {
            Some(call) => Transformed::yes(call),
            None => Transformed::no(Expr::Like(like)),
        },
        Expr::BinaryExpr(BinaryExpr {
            left,
            op: Operator::Or,
            right,
        }) => match merged(&left, &right) {
            Some(call) => Transformed::yes(call),
            None => Transformed::no(Expr::BinaryExpr(BinaryExpr {
                left,
                op: Operator::Or,
                right,
            })),
        },
        other => Transformed::no(other),
    })
}

/// The call a `LIKE` becomes, when it is a plain substring test.
fn call_for(like: &Like) -> Option<Expr> {
    if like.escape_char.is_some() {
        return None;
    }

    let pattern = literal_text(&like.pattern)?;
    let needle = pattern.strip_prefix('%')?.strip_suffix('%')?;

    if needle.is_empty() || needle.contains(['%', '_', '\\']) {
        return None;
    }

    if like.case_insensitive && !needle.is_ascii() {
        return None;
    }

    let call = contains(
        like.expr.as_ref().clone(),
        like.case_insensitive,
        vec![needle.to_owned()],
    );

    Some(if like.negated {
        Expr::Not(Box::new(call))
    } else {
        call
    })
}

/// One call for an `OR` of two calls on the same column with the same case flag.
fn merged(left: &Expr, right: &Expr) -> Option<Expr> {
    let (text, fold, mut needles) = parts(left)?;
    let (other_text, other_fold, more) = parts(right)?;

    if text != other_text || fold != other_fold {
        return None;
    }

    needles.extend(more);
    Some(contains(text.clone(), fold, needles))
}

/// The text, case flag and needles of a [`contains_text`](super) call.
fn parts(expr: &Expr) -> Option<(&Expr, bool, Vec<String>)> {
    let Expr::ScalarFunction(ScalarFunction { func, args }) = expr else {
        return None;
    };

    if func.name() != NAME {
        return None;
    }

    let [
        text,
        Expr::Literal(ScalarValue::Boolean(Some(fold)), _),
        needles @ ..,
    ] = args.as_slice()
    else {
        return None;
    };

    let needles = needles
        .iter()
        .map(literal_text)
        .collect::<Option<Vec<_>>>()?;
    Some((text, *fold, needles))
}

fn contains(text: Expr, fold: bool, needles: Vec<String>) -> Expr {
    let mut args = vec![text, Expr::Literal(ScalarValue::Boolean(Some(fold)), None)];

    args.extend(
        needles
            .into_iter()
            .map(|needle| Expr::Literal(ScalarValue::Utf8(Some(needle)), None)),
    );

    Expr::ScalarFunction(ScalarFunction::new_udf(Arc::new(contains_text_udf()), args))
}

/// A text literal, also behind the cast type coercion may put around it.
fn literal_text(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Literal(value, _) => scalar_text(value),
        Expr::Cast(cast) => literal_text(&cast.expr),
        Expr::TryCast(cast) => literal_text(&cast.expr),
        _ => None,
    }
}
