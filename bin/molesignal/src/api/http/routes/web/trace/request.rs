// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/api/v1/web/trace/{trace_id}` 请求结构（web-investigation-shell）。
//!
//! 当前 path-only 端点，未来加 `?from=&to=` 时把窗口覆盖移到这里。

/// trace_id 校验：长度 1..=128，仅 `[A-Za-z0-9_-]`。
pub fn validate_trace_id(trace_id: &str) -> Result<(), &'static str> {
    if trace_id.is_empty() || trace_id.len() > 128 {
        return Err("trace_id length must be 1..=128");
    }
    if !trace_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("trace_id contains invalid characters");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_quotes_and_semicolons() {
        assert!(validate_trace_id("abc' OR '1'='1").is_err());
        assert!(validate_trace_id("bad;id").is_err());
    }

    #[test]
    fn accepts_hex_and_base32_friendly() {
        assert!(validate_trace_id("abc-123_DEF").is_ok());
        assert!(validate_trace_id("0123456789abcdef0123456789abcdef").is_ok());
    }

    #[test]
    fn enforces_length_bounds() {
        assert!(validate_trace_id("").is_err());
        assert!(validate_trace_id(&"x".repeat(129)).is_err());
    }
}
