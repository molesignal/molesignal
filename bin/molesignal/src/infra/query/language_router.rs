// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 按 `QueryRequest.language` 分发的 [`QueryEngine`]。

use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    domain::query::{PromqlEngine, QueryEngine, QueryLanguage, QueryRequest, QueryResult},
    shared::Result,
};

/// SQL 请求交给 SQL 引擎，PromQL 请求交给 PromQL 引擎。
///
/// 告警评估器这类内部调用方直接用它：评估是系统行为，不走 `QueryService`，因此不受
/// 用户准入控制限流，也不读写面向用户的结果缓存。
pub struct LanguageRoutedEngine {
    sql: Arc<dyn QueryEngine>,
    promql: Arc<dyn PromqlEngine>,
}

impl LanguageRoutedEngine {
    pub fn new(sql: Arc<dyn QueryEngine>, promql: Arc<dyn PromqlEngine>) -> Self {
        Self { sql, promql }
    }
}

#[async_trait]
impl QueryEngine for LanguageRoutedEngine {
    async fn execute(&self, req: QueryRequest) -> Result<QueryResult> {
        match req.language {
            QueryLanguage::Sql => self.sql.execute(req).await,
            QueryLanguage::Promql => self.promql.execute(req).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::{
        ids::Id,
        time::{TimeRange, TimestampMicros},
    };

    /// 两个引擎都返回一行，用 `columns` 标明是谁处理的。
    struct Tagged(&'static str);

    fn result(tag: &str) -> QueryResult {
        QueryResult {
            columns: vec![tag.to_owned()],
            rows: vec![vec![serde_json::json!(1)]],
            scanned_rows: 1,
            took_ms: 0,
            federation: None,
        }
    }

    #[async_trait]
    impl QueryEngine for Tagged {
        async fn execute(&self, _req: QueryRequest) -> Result<QueryResult> {
            Ok(result(self.0))
        }
    }

    #[async_trait]
    impl PromqlEngine for Tagged {
        async fn execute(&self, _req: QueryRequest) -> Result<QueryResult> {
            Ok(result(self.0))
        }
    }

    fn request(language: QueryLanguage) -> QueryRequest {
        QueryRequest {
            org_id: Id::new(),
            language,
            statement: "up".to_owned(),
            time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(1)),
            stream: None,
            limit: Some(1),
            federation_clusters: Vec::new(),
        }
    }

    #[tokio::test]
    async fn dispatches_by_language() {
        let engine = LanguageRoutedEngine::new(Arc::new(Tagged("sql")), Arc::new(Tagged("promql")));

        let sql = engine.execute(request(QueryLanguage::Sql)).await.unwrap();
        let promql = engine
            .execute(request(QueryLanguage::Promql))
            .await
            .unwrap();

        assert_eq!(sql.columns, ["sql"]);
        assert_eq!(promql.columns, ["promql"]);
    }
}
