// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 字段级校验 trait：每个 `*Request` 自己实现 `Validate`，
//! 失败抛 `Error::Invalid("field <X>: <reason>")`。**不引入 third-party `validator`。**

use crate::shared::{Error, Result};

pub trait Validate {
    fn validate(&self) -> Result<()>;
}

/// 非空字符串
pub fn ensure_non_empty(field: &str, v: &str) -> Result<()> {
    if v.trim().is_empty() {
        Err(Error::invalid(format!("field {field}: must not be empty")))
    } else {
        Ok(())
    }
}

/// 字符串长度 ≤ max
pub fn ensure_len_le(field: &str, v: &str, max: usize) -> Result<()> {
    if v.len() > max {
        Err(Error::invalid(format!(
            "field {field}: length {} exceeds max {max}",
            v.len()
        )))
    } else {
        Ok(())
    }
}

/// 整数在 [min, max] 范围
pub fn ensure_range_i64(field: &str, v: i64, min: i64, max: i64) -> Result<()> {
    if v < min || v > max {
        Err(Error::invalid(format!(
            "field {field}: value {v} outside [{min}, {max}]"
        )))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_reject_invalid() {
        assert!(ensure_non_empty("name", "").is_err());
        assert!(ensure_non_empty("name", "  ").is_err());
        assert!(ensure_non_empty("name", "ok").is_ok());
        assert!(ensure_len_le("x", "hello", 3).is_err());
        assert!(ensure_len_le("x", "hi", 3).is_ok());
        assert!(ensure_range_i64("n", 5, 0, 4).is_err());
        assert!(ensure_range_i64("n", 3, 0, 10).is_ok());
    }
}
