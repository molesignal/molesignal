// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/api/v1/web/search` 请求结构（web-investigation-shell）。
//!
//! 与 handler 分离便于 OpenAPI 推导和单测；types-only crate-internal module。

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    /// Comma-separated kind list. Empty / missing → all kinds.
    #[serde(default)]
    pub types: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    20
}

impl SearchQuery {
    /// 委托给 use case 层，别在这里另起一套 —— 两份实现分头演化正是
    /// `validate_query` 长期没人调用的原因。
    pub fn parse_kinds(&self) -> Vec<&str> {
        crate::app::web::search::parse_kinds(self.types.as_deref())
    }

    pub fn capped_limit(&self) -> u32 {
        crate::app::web::search::cap_limit(self.limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_kinds_splits_comma_and_drops_empty() {
        let q = SearchQuery {
            q: "x".into(),
            types: Some("streams,,dashboards,".into()),
            limit: 20,
        };
        assert_eq!(q.parse_kinds(), vec!["streams", "dashboards"]);
    }

    #[test]
    fn capped_limit_caps_to_50() {
        let q = SearchQuery {
            q: "x".into(),
            types: None,
            limit: 100,
        };
        assert_eq!(q.capped_limit(), 50);
    }
}
