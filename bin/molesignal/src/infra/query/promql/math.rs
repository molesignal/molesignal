// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) enum SampleMathParams {
    None,
    Round { nearest: f64 },
    Clamp { min: f64, max: f64 },
    ClampMin { min: f64 },
    ClampMax { max: f64 },
}

pub(super) fn apply_sample_math(
    func: &str,
    input: InstantVector,
    params: SampleMathParams,
) -> Result<InstantVector> {
    let mut items = Vec::with_capacity(input.items.len());
    for (labels, value) in input.items {
        let Some(value) = sample_math_value(func, value, params)? else {
            continue;
        };
        items.push((labels, value));
    }
    Ok(InstantVector { items })
}

pub(super) fn apply_sample_math_range(
    func: &str,
    input: RangeVector,
    params: SampleMathParams,
) -> Result<RangeVector> {
    let mut points = Vec::with_capacity(input.points.len());
    for mut point in input.points {
        let Some(value) = sample_math_value(func, point.value, params)? else {
            continue;
        };
        point.value = value;
        points.push(point);
    }
    Ok(RangeVector { points })
}

pub(super) fn sample_math_value(
    func: &str,
    value: f64,
    params: SampleMathParams,
) -> Result<Option<f64>> {
    let value = match func {
        "abs" => value.abs(),
        "ceil" => value.ceil(),
        "floor" => value.floor(),
        "round" => {
            let SampleMathParams::Round { nearest } = params else {
                return Err(Error::invalid("round requires to_nearest parameter"));
            };
            round_to_nearest(value, nearest)
        }
        "exp" => value.exp(),
        "ln" => value.ln(),
        "log2" => value.log2(),
        "log10" => value.log10(),
        "sqrt" => value.sqrt(),
        "sgn" => sign_sample(value),
        "clamp" => {
            let SampleMathParams::Clamp { min, max } = params else {
                return Err(Error::invalid("clamp requires min and max parameters"));
            };
            let Some(value) = clamp_sample(value, min, max) else {
                return Ok(None);
            };
            value
        }
        "clamp_min" => {
            let SampleMathParams::ClampMin { min } = params else {
                return Err(Error::invalid("clamp_min requires min parameter"));
            };
            clamp_min_sample(value, min)
        }
        "clamp_max" => {
            let SampleMathParams::ClampMax { max } = params else {
                return Err(Error::invalid("clamp_max requires max parameter"));
            };
            clamp_max_sample(value, max)
        }
        "sin" => value.sin(),
        "cos" => value.cos(),
        "tan" => value.tan(),
        "asin" => value.asin(),
        "acos" => value.acos(),
        "atan" => value.atan(),
        "sinh" => value.sinh(),
        "cosh" => value.cosh(),
        "tanh" => value.tanh(),
        "asinh" => value.asinh(),
        "acosh" => value.acosh(),
        "atanh" => value.atanh(),
        "deg" => value * 180.0 / std::f64::consts::PI,
        "rad" => value * std::f64::consts::PI / 180.0,
        other => {
            return Err(Error::invalid(format!(
                "promql function not yet supported: {other}"
            )));
        }
    };
    Ok(Some(value))
}

pub(super) fn round_to_nearest(value: f64, nearest: f64) -> f64 {
    ((value / nearest) + 0.5).floor() * nearest
}

pub(super) fn sign_sample(value: f64) -> f64 {
    if value.is_nan() {
        f64::NAN
    } else if value > 0.0 {
        1.0
    } else if value < 0.0 {
        -1.0
    } else {
        0.0
    }
}

pub(super) fn clamp_sample(value: f64, min: f64, max: f64) -> Option<f64> {
    if min > max {
        return None;
    }
    if min.is_nan() || max.is_nan() || value.is_nan() {
        return Some(f64::NAN);
    }
    Some(value.max(min).min(max))
}

pub(super) fn clamp_min_sample(value: f64, min: f64) -> f64 {
    if min.is_nan() || value.is_nan() {
        f64::NAN
    } else {
        value.max(min)
    }
}

pub(super) fn clamp_max_sample(value: f64, max: f64) -> f64 {
    if max.is_nan() || value.is_nan() {
        f64::NAN
    } else {
        value.min(max)
    }
}
