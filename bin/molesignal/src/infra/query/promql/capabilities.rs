// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PromQL editor capabilities exposed to API clients.
//!
//! Keep evaluator membership checks and completion metadata on the same list so
//! the UI never advertises functions that this engine rejects.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromqlCompletionKind {
    Function,
    Aggregation,
    Keyword,
    Operator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PromqlCompletion {
    pub label: &'static str,
    pub insert_text: &'static str,
    pub detail: &'static str,
    pub documentation: &'static str,
    pub kind: PromqlCompletionKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromqlCapabilities {
    pub engine: &'static str,
    pub version: u8,
    pub functions: Vec<PromqlCompletion>,
    pub aggregations: Vec<PromqlCompletion>,
    pub keywords: Vec<PromqlCompletion>,
    pub operators: Vec<PromqlCompletion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FunctionCategory {
    Rate,
    OverTime,
    RangeVector,
    Histogram,
    Label,
    Sort,
    Presence,
    Time,
    DateTime,
    Conversion,
    Constant,
    Math,
}

#[derive(Debug, Clone, Copy)]
struct FunctionSpec {
    completion: PromqlCompletion,
    category: FunctionCategory,
}

macro_rules! function {
    ($name:literal, $signature:literal, $insert:literal, $category:ident, $docs:literal) => {
        FunctionSpec {
            completion: PromqlCompletion {
                label: $name,
                insert_text: $insert,
                detail: $signature,
                documentation: $docs,
                kind: PromqlCompletionKind::Function,
            },
            category: FunctionCategory::$category,
        }
    };
}

macro_rules! completion {
    ($label:literal, $insert:literal, $detail:literal, $docs:literal, $kind:ident) => {
        PromqlCompletion {
            label: $label,
            insert_text: $insert,
            detail: $detail,
            documentation: $docs,
            kind: PromqlCompletionKind::$kind,
        }
    };
}

const FUNCTION_SPECS: &[FunctionSpec] = &[
    function!(
        "rate",
        "rate(range-vector)",
        "rate(${1:metric}[${2:5m}])",
        Rate,
        "Per-second average rate over a range vector."
    ),
    function!(
        "irate",
        "irate(range-vector)",
        "irate(${1:metric}[${2:5m}])",
        Rate,
        "Instantaneous per-second rate from the last two samples."
    ),
    function!(
        "increase",
        "increase(range-vector)",
        "increase(${1:metric}[${2:5m}])",
        Rate,
        "Total increase over a range vector."
    ),
    function!(
        "avg_over_time",
        "avg_over_time(range-vector)",
        "avg_over_time(${1:metric}[${2:5m}])",
        OverTime,
        "Average of samples in the selected time window."
    ),
    function!(
        "min_over_time",
        "min_over_time(range-vector)",
        "min_over_time(${1:metric}[${2:5m}])",
        OverTime,
        "Minimum sample in the selected time window."
    ),
    function!(
        "max_over_time",
        "max_over_time(range-vector)",
        "max_over_time(${1:metric}[${2:5m}])",
        OverTime,
        "Maximum sample in the selected time window."
    ),
    function!(
        "sum_over_time",
        "sum_over_time(range-vector)",
        "sum_over_time(${1:metric}[${2:5m}])",
        OverTime,
        "Sum of samples in the selected time window."
    ),
    function!(
        "count_over_time",
        "count_over_time(range-vector)",
        "count_over_time(${1:metric}[${2:5m}])",
        OverTime,
        "Number of samples in the selected time window."
    ),
    function!(
        "quantile_over_time",
        "quantile_over_time(scalar, range-vector)",
        "quantile_over_time(${1:0.95}, ${2:metric}[${3:5m}])",
        OverTime,
        "Quantile of samples in the selected time window."
    ),
    function!(
        "stddev_over_time",
        "stddev_over_time(range-vector)",
        "stddev_over_time(${1:metric}[${2:5m}])",
        OverTime,
        "Population standard deviation over a time window."
    ),
    function!(
        "stdvar_over_time",
        "stdvar_over_time(range-vector)",
        "stdvar_over_time(${1:metric}[${2:5m}])",
        OverTime,
        "Population variance over a time window."
    ),
    function!(
        "last_over_time",
        "last_over_time(range-vector)",
        "last_over_time(${1:metric}[${2:5m}])",
        OverTime,
        "Most recent sample in the selected time window."
    ),
    function!(
        "present_over_time",
        "present_over_time(range-vector)",
        "present_over_time(${1:metric}[${2:5m}])",
        OverTime,
        "Returns 1 when the range contains at least one sample."
    ),
    function!(
        "mad_over_time",
        "mad_over_time(range-vector)",
        "mad_over_time(${1:metric}[${2:5m}])",
        OverTime,
        "Median absolute deviation over a time window."
    ),
    function!(
        "delta",
        "delta(range-vector)",
        "delta(${1:metric}[${2:5m}])",
        RangeVector,
        "Difference between the first and last samples."
    ),
    function!(
        "idelta",
        "idelta(range-vector)",
        "idelta(${1:metric}[${2:5m}])",
        RangeVector,
        "Difference between the last two samples."
    ),
    function!(
        "deriv",
        "deriv(range-vector)",
        "deriv(${1:metric}[${2:5m}])",
        RangeVector,
        "Per-second derivative calculated by linear regression."
    ),
    function!(
        "predict_linear",
        "predict_linear(range-vector, seconds)",
        "predict_linear(${1:metric}[${2:5m}], ${3:3600})",
        RangeVector,
        "Predicts a future value using linear regression."
    ),
    function!(
        "resets",
        "resets(range-vector)",
        "resets(${1:metric}[${2:5m}])",
        RangeVector,
        "Counts counter resets in a time window."
    ),
    function!(
        "changes",
        "changes(range-vector)",
        "changes(${1:metric}[${2:5m}])",
        RangeVector,
        "Counts value changes in a time window."
    ),
    function!(
        "holt_winters",
        "holt_winters(range-vector, sf, tf)",
        "holt_winters(${1:metric}[${2:5m}], ${3:0.2}, ${4:0.1})",
        RangeVector,
        "Double exponential smoothing using smoothing and trend factors."
    ),
    function!(
        "double_exponential_smoothing",
        "double_exponential_smoothing(range-vector, sf, tf)",
        "double_exponential_smoothing(${1:metric}[${2:5m}], ${3:0.2}, ${4:0.1})",
        RangeVector,
        "Alias for Holt-Winters double exponential smoothing."
    ),
    function!(
        "histogram_quantile",
        "histogram_quantile(q, vector)",
        "histogram_quantile(${1:0.95}, ${2:vector})",
        Histogram,
        "Calculates a quantile from classic histogram buckets."
    ),
    function!(
        "histogram_fraction",
        "histogram_fraction(lower, upper, vector)",
        "histogram_fraction(${1:0}, ${2:1}, ${3:vector})",
        Histogram,
        "Estimates the fraction of observations between two bounds."
    ),
    function!(
        "label_replace",
        "label_replace(vector, dst, replacement, src, regex)",
        "label_replace(${1:vector}, \"${2:dst}\", \"${3:replacement}\", \"${4:src}\", \"${5:regex}\")",
        Label,
        "Rewrites a label using a regular-expression match."
    ),
    function!(
        "label_join",
        "label_join(vector, dst, separator, src...)",
        "label_join(${1:vector}, \"${2:dst}\", \"${3:-}\", \"${4:src}\")",
        Label,
        "Joins source labels into a destination label."
    ),
    function!(
        "sort",
        "sort(vector)",
        "sort(${1:vector})",
        Sort,
        "Sorts samples by value in ascending order."
    ),
    function!(
        "sort_desc",
        "sort_desc(vector)",
        "sort_desc(${1:vector})",
        Sort,
        "Sorts samples by value in descending order."
    ),
    function!(
        "sort_by_label",
        "sort_by_label(vector, label...)",
        "sort_by_label(${1:vector}, \"${2:label}\")",
        Sort,
        "Sorts samples lexicographically by one or more labels."
    ),
    function!(
        "sort_by_label_desc",
        "sort_by_label_desc(vector, label...)",
        "sort_by_label_desc(${1:vector}, \"${2:label}\")",
        Sort,
        "Sorts samples by labels in descending order."
    ),
    function!(
        "absent",
        "absent(vector)",
        "absent(${1:vector})",
        Presence,
        "Returns 1 when the input vector has no samples."
    ),
    function!(
        "absent_over_time",
        "absent_over_time(range-vector)",
        "absent_over_time(${1:metric}[${2:5m}])",
        Presence,
        "Returns 1 when a time window has no samples."
    ),
    function!(
        "time",
        "time()",
        "time()",
        Time,
        "Returns the evaluation time as Unix seconds."
    ),
    function!(
        "timestamp",
        "timestamp(vector)",
        "timestamp(${1:vector})",
        Time,
        "Returns sample timestamps as Unix seconds."
    ),
    function!(
        "minute",
        "minute(vector?)",
        "minute(${1:vector})",
        DateTime,
        "Extracts the UTC minute from sample values interpreted as Unix time."
    ),
    function!(
        "hour",
        "hour(vector?)",
        "hour(${1:vector})",
        DateTime,
        "Extracts the UTC hour from sample values interpreted as Unix time."
    ),
    function!(
        "day_of_week",
        "day_of_week(vector?)",
        "day_of_week(${1:vector})",
        DateTime,
        "Returns the UTC weekday where Sunday is 0."
    ),
    function!(
        "day_of_month",
        "day_of_month(vector?)",
        "day_of_month(${1:vector})",
        DateTime,
        "Returns the UTC day of month."
    ),
    function!(
        "day_of_year",
        "day_of_year(vector?)",
        "day_of_year(${1:vector})",
        DateTime,
        "Returns the UTC day of year."
    ),
    function!(
        "days_in_month",
        "days_in_month(vector?)",
        "days_in_month(${1:vector})",
        DateTime,
        "Returns the number of days in the UTC month."
    ),
    function!(
        "month",
        "month(vector?)",
        "month(${1:vector})",
        DateTime,
        "Returns the UTC month number."
    ),
    function!(
        "year",
        "year(vector?)",
        "year(${1:vector})",
        DateTime,
        "Returns the UTC year."
    ),
    function!(
        "vector",
        "vector(scalar)",
        "vector(${1:scalar})",
        Conversion,
        "Converts a scalar value to an instant vector."
    ),
    function!(
        "scalar",
        "scalar(vector)",
        "scalar(${1:vector})",
        Conversion,
        "Converts a single-sample vector to a scalar."
    ),
    function!(
        "pi",
        "pi()",
        "pi()",
        Constant,
        "Returns the mathematical constant pi."
    ),
    function!(
        "abs",
        "abs(vector)",
        "abs(${1:vector})",
        Math,
        "Absolute value."
    ),
    function!(
        "ceil",
        "ceil(vector)",
        "ceil(${1:vector})",
        Math,
        "Rounds values up."
    ),
    function!(
        "floor",
        "floor(vector)",
        "floor(${1:vector})",
        Math,
        "Rounds values down."
    ),
    function!(
        "round",
        "round(vector, nearest?)",
        "round(${1:vector}, ${2:1})",
        Math,
        "Rounds values to the nearest multiple."
    ),
    function!(
        "exp",
        "exp(vector)",
        "exp(${1:vector})",
        Math,
        "Natural exponential."
    ),
    function!(
        "ln",
        "ln(vector)",
        "ln(${1:vector})",
        Math,
        "Natural logarithm."
    ),
    function!(
        "log2",
        "log2(vector)",
        "log2(${1:vector})",
        Math,
        "Base-2 logarithm."
    ),
    function!(
        "log10",
        "log10(vector)",
        "log10(${1:vector})",
        Math,
        "Base-10 logarithm."
    ),
    function!(
        "sqrt",
        "sqrt(vector)",
        "sqrt(${1:vector})",
        Math,
        "Square root."
    ),
    function!(
        "sgn",
        "sgn(vector)",
        "sgn(${1:vector})",
        Math,
        "Sign of each sample."
    ),
    function!(
        "clamp",
        "clamp(vector, min, max)",
        "clamp(${1:vector}, ${2:min}, ${3:max})",
        Math,
        "Clamps samples to an inclusive range."
    ),
    function!(
        "clamp_min",
        "clamp_min(vector, min)",
        "clamp_min(${1:vector}, ${2:min})",
        Math,
        "Clamps samples to a minimum value."
    ),
    function!(
        "clamp_max",
        "clamp_max(vector, max)",
        "clamp_max(${1:vector}, ${2:max})",
        Math,
        "Clamps samples to a maximum value."
    ),
    function!(
        "sin",
        "sin(vector)",
        "sin(${1:vector})",
        Math,
        "Sine in radians."
    ),
    function!(
        "cos",
        "cos(vector)",
        "cos(${1:vector})",
        Math,
        "Cosine in radians."
    ),
    function!(
        "tan",
        "tan(vector)",
        "tan(${1:vector})",
        Math,
        "Tangent in radians."
    ),
    function!(
        "asin",
        "asin(vector)",
        "asin(${1:vector})",
        Math,
        "Inverse sine."
    ),
    function!(
        "acos",
        "acos(vector)",
        "acos(${1:vector})",
        Math,
        "Inverse cosine."
    ),
    function!(
        "atan",
        "atan(vector)",
        "atan(${1:vector})",
        Math,
        "Inverse tangent."
    ),
    function!(
        "sinh",
        "sinh(vector)",
        "sinh(${1:vector})",
        Math,
        "Hyperbolic sine."
    ),
    function!(
        "cosh",
        "cosh(vector)",
        "cosh(${1:vector})",
        Math,
        "Hyperbolic cosine."
    ),
    function!(
        "tanh",
        "tanh(vector)",
        "tanh(${1:vector})",
        Math,
        "Hyperbolic tangent."
    ),
    function!(
        "asinh",
        "asinh(vector)",
        "asinh(${1:vector})",
        Math,
        "Inverse hyperbolic sine."
    ),
    function!(
        "acosh",
        "acosh(vector)",
        "acosh(${1:vector})",
        Math,
        "Inverse hyperbolic cosine."
    ),
    function!(
        "atanh",
        "atanh(vector)",
        "atanh(${1:vector})",
        Math,
        "Inverse hyperbolic tangent."
    ),
    function!(
        "deg",
        "deg(vector)",
        "deg(${1:vector})",
        Math,
        "Converts radians to degrees."
    ),
    function!(
        "rad",
        "rad(vector)",
        "rad(${1:vector})",
        Math,
        "Converts degrees to radians."
    ),
];

const AGGREGATIONS: &[PromqlCompletion] = &[
    completion!(
        "sum",
        "sum by (${1:label}) (${2:vector})",
        "sum by (labels) (vector)",
        "Sums samples by label group.",
        Aggregation
    ),
    completion!(
        "avg",
        "avg by (${1:label}) (${2:vector})",
        "avg by (labels) (vector)",
        "Averages samples by label group.",
        Aggregation
    ),
    completion!(
        "min",
        "min by (${1:label}) (${2:vector})",
        "min by (labels) (vector)",
        "Selects the minimum sample per group.",
        Aggregation
    ),
    completion!(
        "max",
        "max by (${1:label}) (${2:vector})",
        "max by (labels) (vector)",
        "Selects the maximum sample per group.",
        Aggregation
    ),
    completion!(
        "count",
        "count by (${1:label}) (${2:vector})",
        "count by (labels) (vector)",
        "Counts samples per group.",
        Aggregation
    ),
    completion!(
        "stddev",
        "stddev by (${1:label}) (${2:vector})",
        "stddev by (labels) (vector)",
        "Population standard deviation per group.",
        Aggregation
    ),
    completion!(
        "stdvar",
        "stdvar by (${1:label}) (${2:vector})",
        "stdvar by (labels) (vector)",
        "Population variance per group.",
        Aggregation
    ),
    completion!(
        "quantile",
        "quantile(${1:0.95}, ${2:vector})",
        "quantile(q, vector)",
        "Calculates a quantile across samples.",
        Aggregation
    ),
    completion!(
        "group",
        "group by (${1:label}) (${2:vector})",
        "group by (labels) (vector)",
        "Returns 1 for every populated group.",
        Aggregation
    ),
    completion!(
        "count_values",
        "count_values(\"${1:value}\", ${2:vector})",
        "count_values(label, vector)",
        "Counts occurrences of each sample value.",
        Aggregation
    ),
    completion!(
        "topk",
        "topk(${1:5}, ${2:vector})",
        "topk(k, vector)",
        "Returns the largest k samples.",
        Aggregation
    ),
    completion!(
        "bottomk",
        "bottomk(${1:5}, ${2:vector})",
        "bottomk(k, vector)",
        "Returns the smallest k samples.",
        Aggregation
    ),
    completion!(
        "limitk",
        "limitk(${1:5}, ${2:vector})",
        "limitk(k, vector)",
        "Returns a deterministic subset of k samples.",
        Aggregation
    ),
    completion!(
        "limit_ratio",
        "limit_ratio(${1:0.1}, ${2:vector})",
        "limit_ratio(ratio, vector)",
        "Returns a deterministic ratio of samples.",
        Aggregation
    ),
];

const KEYWORDS: &[PromqlCompletion] = &[
    completion!(
        "by",
        "by (${1:label})",
        "aggregation modifier",
        "Keeps the listed labels when aggregating.",
        Keyword
    ),
    completion!(
        "without",
        "without (${1:label})",
        "aggregation modifier",
        "Drops the listed labels when aggregating.",
        Keyword
    ),
    completion!(
        "on",
        "on (${1:label})",
        "vector matching",
        "Matches vectors only on the listed labels.",
        Keyword
    ),
    completion!(
        "ignoring",
        "ignoring (${1:label})",
        "vector matching",
        "Ignores the listed labels during vector matching.",
        Keyword
    ),
    completion!(
        "group_left",
        "group_left (${1:label})",
        "vector matching",
        "Enables many-to-one matching and copies labels from the right.",
        Keyword
    ),
    completion!(
        "group_right",
        "group_right (${1:label})",
        "vector matching",
        "Enables one-to-many matching and copies labels from the left.",
        Keyword
    ),
    completion!(
        "bool",
        "bool",
        "comparison modifier",
        "Returns 0 or 1 instead of filtering comparison results.",
        Keyword
    ),
    completion!(
        "offset",
        "offset ${1:5m}",
        "selector modifier",
        "Offsets a selector relative to the evaluation time.",
        Keyword
    ),
];

const OPERATORS: &[PromqlCompletion] = &[
    completion!(
        "and",
        "and",
        "set operator",
        "Intersection of two instant vectors.",
        Operator
    ),
    completion!(
        "or",
        "or",
        "set operator",
        "Union of two instant vectors.",
        Operator
    ),
    completion!(
        "unless",
        "unless",
        "set operator",
        "Left-side samples with no right-side match.",
        Operator
    ),
    completion!(
        "=",
        "=",
        "label matcher",
        "Exact label-value matcher.",
        Operator
    ),
    completion!(
        "=~",
        "=~",
        "label matcher",
        "Regular-expression label matcher.",
        Operator
    ),
    completion!(
        "!~",
        "!~",
        "label matcher",
        "Negative regular-expression label matcher.",
        Operator
    ),
    completion!("+", "+", "arithmetic operator", "Addition.", Operator),
    completion!("-", "-", "arithmetic operator", "Subtraction.", Operator),
    completion!("*", "*", "arithmetic operator", "Multiplication.", Operator),
    completion!("/", "/", "arithmetic operator", "Division.", Operator),
    completion!("%", "%", "arithmetic operator", "Modulo.", Operator),
    completion!("^", "^", "arithmetic operator", "Exponentiation.", Operator),
    completion!(
        "==",
        "==",
        "comparison operator",
        "Equal comparison.",
        Operator
    ),
    completion!(
        "!=",
        "!=",
        "comparison / label matcher",
        "Not-equal comparison or label-value matcher.",
        Operator
    ),
    completion!(
        ">",
        ">",
        "comparison operator",
        "Greater-than comparison.",
        Operator
    ),
    completion!(
        "<",
        "<",
        "comparison operator",
        "Less-than comparison.",
        Operator
    ),
    completion!(
        ">=",
        ">=",
        "comparison operator",
        "Greater-than-or-equal comparison.",
        Operator
    ),
    completion!(
        "<=",
        "<=",
        "comparison operator",
        "Less-than-or-equal comparison.",
        Operator
    ),
];

pub fn capabilities() -> PromqlCapabilities {
    PromqlCapabilities {
        engine: "molesignal-promql",
        version: 1,
        functions: FUNCTION_SPECS.iter().map(|spec| spec.completion).collect(),
        aggregations: AGGREGATIONS.to_vec(),
        keywords: KEYWORDS.to_vec(),
        operators: OPERATORS.to_vec(),
    }
}

pub(super) fn is_function_category(name: &str, category: FunctionCategory) -> bool {
    FUNCTION_SPECS
        .iter()
        .any(|spec| spec.category == category && spec.completion.label == name)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn completion_labels_are_unique_within_each_section() {
        let capabilities = capabilities();
        for items in [
            capabilities.functions,
            capabilities.aggregations,
            capabilities.keywords,
            capabilities.operators,
        ] {
            let mut labels = BTreeSet::new();
            for item in items {
                assert!(
                    labels.insert(item.label),
                    "duplicate capability: {}",
                    item.label
                );
            }
        }
    }

    #[test]
    fn capability_list_exposes_supported_groups_only() {
        assert!(is_function_category("rate", FunctionCategory::Rate));
        assert!(is_function_category(
            "quantile_over_time",
            FunctionCategory::OverTime
        ));
        assert!(is_function_category("clamp", FunctionCategory::Math));
        assert!(is_function_category("pi", FunctionCategory::Constant));
        assert!(!is_function_category("pi", FunctionCategory::Math));
        assert!(
            !capabilities()
                .functions
                .iter()
                .any(|item| item.label == "histogram_avg")
        );
    }
}
