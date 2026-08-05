// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Web search use case helpers（web-investigation-shell）。
//!
//! 纯函数：query 字符串 → trigram-safe min length 校验 + kind 列表解析。
//! 真实 repo 调用在 handler 中。

const MIN_QUERY_CHARS: usize = 1;
const MAX_QUERY_CHARS: usize = 256;
const HARD_CAP_LIMIT: u32 = 50;

pub fn cap_limit(limit: u32) -> u32 {
    limit.min(HARD_CAP_LIMIT)
}

pub fn parse_kinds(types: Option<&str>) -> Vec<&str> {
    types
        .map(|s| s.split(',').filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

pub fn validate_query(q: &str) -> Result<(), &'static str> {
    let len = q.chars().count();
    if len < MIN_QUERY_CHARS {
        return Err("query must be at least 1 character");
    }
    if len > MAX_QUERY_CHARS {
        return Err("query too long (max 256 characters)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_kinds_filters_empty_segments() {
        assert_eq!(
            parse_kinds(Some("streams,,dashboards,")),
            vec!["streams", "dashboards"]
        );
        assert_eq!(parse_kinds(None), Vec::<&str>::new());
    }

    #[test]
    fn cap_limit_clamps_high_values() {
        assert_eq!(cap_limit(0), 0);
        assert_eq!(cap_limit(20), 20);
        assert_eq!(cap_limit(999), HARD_CAP_LIMIT);
    }

    #[test]
    fn validate_query_rejects_extremes() {
        assert!(validate_query("").is_err());
        assert!(validate_query("a").is_ok());
        assert!(validate_query(&"x".repeat(257)).is_err());
    }
}
