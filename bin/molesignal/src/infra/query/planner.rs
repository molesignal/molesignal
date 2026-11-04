// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 多租户 planner rewrite。
//!
//! 当前实现：我们的查询路径**by construction** 已是多租户安全的：
//! - [`super::super::search::datafusion_engine::DataFusionEngine`] 跑查询前用
//!   `QueryFileSource::find(org_id, ...)` 只拉当前 org 的候选文件
//! - 注册到 `SessionContext` 的 Parquet `TableProvider` 只含当前 org 的候选文件
//! - SQL 中即使写 `FROM streamX`，DataFusion 也只能看到当前 org 注册的那张表
//! - 数据文件经 [`crate::infra::storage::org_scoped_store::scoped_to_org`] 读取，只能访问本 org
//!   的对象前缀；即使候选里混入别的 org 的 key，读取也会被存储层拒绝
//!
//! 因此 [`RewriteTableNamesPass`] 当前是**透传**（不修改 LogicalPlan），但保留 API
//! 形态便于将来切到 DataFusion 多 catalog / 共享 ctx 时插入真正的 `Filter(org_id = ...)`
//! 注入。
//!
//! 对应 spec "stream 不存在/属其它 org → Forbidden" 由 DataFusionEngine
//! 在 execute 之前调 [`ensure_stream_in_org`] 校验。

use crate::{
    domain::stream::{StreamRepository, StreamType},
    shared::{Error, Result, ids::Id, time::TimeRange},
};

/// 抓取 SQL 中所有 base table 名。
///
/// **Deprecated**：自 `sqlparser-join-planner` change 起，新代码请直接调
/// [`crate::infra::query::parser::extract_referenced_tables`]，它返回 `Vec<TableRef>`
/// 还带 alias / schema 信息。本函数现转发到新解析器并丢弃额外字段，仅维持旧 API
/// 的二进制兼容。解析失败时返空 vec（保持旧行为：宽容地把"没有可识别 FROM"作为
/// 空集，让 DataFusion 自己抛 syntax error）。
#[deprecated(
    note = "use `crate::infra::query::parser::extract_referenced_tables` instead; \
            returns TableRef with alias / schema and uses sqlparser AST"
)]
pub fn parse_from_tables(stmt: &str) -> Vec<String> {
    let mut out: Vec<String> = crate::infra::query::parser::extract_referenced_tables(stmt)
        .map(|refs| refs.into_iter().map(|r| r.name).collect())
        .unwrap_or_default();

    // 旧 API 排序后返；保持现有 caller 的稳定输出。
    out.sort();
    out.dedup();
    out
}

/// 校验 streamHint 在当前 org 下可被查询，查询时间范围 `time_range` 也在它允许的范围内。
/// - `_audit` 只给持有 `audit.read` 的调用者读（[`super::grants`]）→ 否则 `Error::Forbidden`。
/// - stream 不存在或属其它 org → `Error::Forbidden("stream not found: <name>")`。
/// - stream 存在但被标记为不可查询（`settings.queryable == false`，仅作 intake 入口 /
///   pipeline 源）→ `Error::Forbidden("stream is not queryable: <name>")`——与「不存在」
///   分开返回，便于调用方/前端区分原因。
/// - 时间范围长于 stream 设置的 `max_query_range_hours` → `Error::InvalidArgument`。
pub async fn ensure_stream_in_org(
    streams: &dyn StreamRepository,
    org_id: &Id,
    name: &str,
    stream_type: StreamType,
    time_range: TimeRange,
) -> Result<()> {
    if !super::grants::may_read(name) {
        return Err(Error::forbidden(format!(
            "reading `{name}` needs the {} permission",
            super::grants::AUDIT_READ
        )));
    }

    let def = match streams.get(org_id, name, stream_type).await {
        Ok(def) => def,
        Err(Error::NotFound(_)) => {
            return Err(Error::forbidden(format!("stream not found: {name}")));
        }
        Err(e) => return Err(e),
    };

    let settings = streams.get_settings(&def.id).await?;

    if !settings.queryable {
        return Err(Error::forbidden(format!("stream is not queryable: {name}")));
    }

    if let Some(hours) = settings.exceeded_query_range(time_range) {
        return Err(query_range_error(name, hours));
    }

    Ok(())
}

/// 查询时间范围长于数据流允许的 `hours` 小时。
pub fn query_range_error(name: &str, hours: u32) -> Error {
    Error::invalid(format!(
        "stream `{name}` allows queries over at most {hours} hours; narrow the time range"
    ))
}

/// LogicalPlan rewrite pass（占位 — 当前透传）。
pub struct RewriteTableNamesPass {
    pub org_id: Id,
}

impl RewriteTableNamesPass {
    pub fn new(org_id: Id) -> Self {
        Self { org_id }
    }

    /// 对 LogicalPlan 应用 rewrite。当前直接返回原 plan（参见模块文档）。
    ///
    /// ⚠️ **零生产调用者**：本方法未接入任何查询路径，不承担隔离职责。org 隔离由
    /// `QueryFileSource::find(org_id, ...)` by-construction 成立（见模块文档）。保留
    /// 此 API 仅为将来切 DataFusion 多 catalog 时预留插入点，勿误信这里已在过滤。
    pub fn apply<P>(&self, plan: P) -> P {
        // 占位：DataFusion LogicalPlan rewriter 接入留至后续阶段。
        let _ = &self.org_id;
        plan
    }
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;

    #[test]
    fn parse_from_single() {
        let r = parse_from_tables("SELECT * FROM logs WHERE x = 1");
        assert_eq!(r, vec!["logs".to_string()]);
    }

    #[test]
    fn parse_join_picks_up_both() {
        let r = parse_from_tables(
            "SELECT l.msg, t.trace_id FROM logs l JOIN traces t ON l.trace_id = t.trace_id",
        );
        assert_eq!(r, vec!["logs".to_string(), "traces".to_string()]);
    }

    #[test]
    fn parse_multiple_join_chain() {
        let r = parse_from_tables("SELECT * FROM a JOIN b ON a.id=b.aid JOIN c ON b.id=c.bid");
        assert_eq!(r, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn parse_handles_subquery_from() {
        // 子查询里也是 FROM；先全部抓回来再 dedup
        let r = parse_from_tables(
            "SELECT * FROM (SELECT id FROM logs) sub JOIN traces ON sub.id = traces.id",
        );
        assert!(r.contains(&"logs".to_string()));
        assert!(r.contains(&"traces".to_string()));
    }

    // ---- ensure_stream_in_org: 存在性 + queryable 闸门 ----

    use async_trait::async_trait;

    use crate::{
        domain::stream::{Schema, StreamDefinition, StreamRepository, StreamSettings},
        shared::time::TimestampMicros,
    };

    struct FakeStreams {
        exists: bool,
        queryable: bool,
        max_query_range_hours: Option<u32>,
    }

    #[async_trait]
    impl StreamRepository for FakeStreams {
        async fn create(&self, def: StreamDefinition) -> Result<StreamDefinition> {
            Ok(def)
        }
        async fn update_schema(&self, _id: &Id, _schema: Schema) -> Result<()> {
            Ok(())
        }
        async fn get(
            &self,
            org_id: &Id,
            name: &str,
            stream_type: StreamType,
        ) -> Result<StreamDefinition> {
            if !self.exists {
                return Err(Error::not_found(format!("stream {name}")));
            }
            Ok(StreamDefinition {
                id: Id::from_string("stream-1"),
                org_id: org_id.clone(),
                name: name.to_string(),
                stream_type,
                schema: Schema { fields: vec![] },
                retention: None,
                created_at: TimestampMicros(0),
                updated_at: TimestampMicros(0),
            })
        }
        async fn list(&self, _org_id: &Id) -> Result<Vec<StreamDefinition>> {
            Ok(vec![])
        }
        async fn get_settings(&self, _id: &Id) -> Result<StreamSettings> {
            Ok(StreamSettings {
                queryable: self.queryable,
                max_query_range_hours: self.max_query_range_hours,
                ..Default::default()
            })
        }
        async fn delete(&self, _id: &Id) -> Result<()> {
            Ok(())
        }
    }

    fn hours(hours: i64) -> TimeRange {
        TimeRange::new(TimestampMicros(0), TimestampMicros(hours * 3_600_000_000))
    }

    #[tokio::test]
    async fn queryable_stream_passes() {
        let streams = FakeStreams {
            exists: true,
            queryable: true,
            max_query_range_hours: None,
        };
        ensure_stream_in_org(
            &streams,
            &Id::from_string("org"),
            "app",
            StreamType::LOGS,
            hours(24 * 365),
        )
        .await
        .expect("queryable stream should pass");
    }

    #[tokio::test]
    async fn the_audit_trail_is_read_only_with_the_audit_grant() {
        use crate::infra::query::grants::{ReadGrants, scope};

        let streams = FakeStreams {
            exists: true,
            queryable: true,
            max_query_range_hours: None,
        };
        let org = Id::from_string("org");
        let read = || ensure_stream_in_org(&streams, &org, "_audit", StreamType::LOGS, hours(1));

        let refused = read().await.unwrap_err();
        assert!(matches!(refused, Error::Forbidden(_)), "{refused:?}");
        assert!(refused.to_string().contains("audit.read"));

        scope(ReadGrants::of_caller(true), read())
            .await
            .expect("a caller with audit.read reads the audit trail");
    }

    #[tokio::test]
    async fn a_range_longer_than_the_stream_allows_is_rejected() {
        let streams = FakeStreams {
            exists: true,
            queryable: true,
            max_query_range_hours: Some(24),
        };
        let org = Id::from_string("org");

        ensure_stream_in_org(&streams, &org, "app", StreamType::LOGS, hours(24))
            .await
            .expect("a day is allowed");

        let err = ensure_stream_in_org(&streams, &org, "app", StreamType::LOGS, hours(25))
            .await
            .expect_err("more than a day is not");

        assert!(matches!(err, Error::InvalidArgument(_)), "{err:?}");
        assert!(err.to_string().contains("24 hours"), "{err}");
    }

    #[tokio::test]
    async fn non_queryable_stream_is_forbidden_distinctly() {
        let streams = FakeStreams {
            exists: true,
            queryable: false,
            max_query_range_hours: None,
        };
        let err = ensure_stream_in_org(
            &streams,
            &Id::from_string("org"),
            "app",
            StreamType::LOGS,
            hours(1),
        )
        .await
        .expect_err("non-queryable stream must be rejected");
        match err {
            Error::Forbidden(msg) => {
                // 与「不存在」区分开：专门的 not-queryable 信号。
                assert!(msg.contains("not queryable"), "unexpected message: {msg}");
            }
            other => panic!("expected Forbidden, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn missing_stream_is_forbidden_not_found() {
        let streams = FakeStreams {
            exists: false,
            queryable: true,
            max_query_range_hours: None,
        };
        let err = ensure_stream_in_org(
            &streams,
            &Id::from_string("org"),
            "app",
            StreamType::LOGS,
            hours(1),
        )
        .await
        .expect_err("missing stream must be rejected");
        match err {
            Error::Forbidden(msg) => {
                assert!(msg.contains("not found"), "unexpected message: {msg}")
            }
            other => panic!("expected Forbidden, got {other:?}"),
        }
    }
}
