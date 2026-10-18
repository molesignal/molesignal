// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 模型调用统计查询。

use super::AGENT_STREAM;
use crate::shared::ids::Id;

/// 用量流里只有部分调用才写的列：没填单价的模型服务不写 `cost` 与 `cost_currency`，根因分析
/// 与 LLM 函数的调用不写 `user_id`。一列从没写过时不在流的 schema 里，SQL 引用它会无法规划，
/// 所以查询前按 schema 决定引用它还是换成 NULL。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OptionalColumns {
    pub cost: bool,
    pub user_id: bool,
}

impl OptionalColumns {
    /// 从流 schema 的字段名得出。
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        let mut columns = Self::default();

        for name in names {
            match name {
                "cost_currency" => columns.cost = true,
                "user_id" => columns.user_id = true,
                _ => {}
            }
        }

        columns
    }

    /// 合计成本；从没有调用算过成本时为 NULL，不是 0。
    fn cost(self) -> &'static str {
        if self.cost {
            "sum(cost)"
        } else {
            "CAST(NULL AS DOUBLE)"
        }
    }

    /// 成本的币种：不同币种的成本分行合计，不能加在一起。
    fn currency(self) -> &'static str {
        if self.cost {
            "cost_currency"
        } else {
            "CAST(NULL AS VARCHAR)"
        }
    }
}

pub struct AgentStatsQuery {
    pub org_id: Id,
    pub from_micros: i64,
    pub to_micros: i64,
    pub columns: OptionalColumns,
}

impl AgentStatsQuery {
    /// 合计，每个币种一行；没有单价的调用在币种为空的那行。
    pub fn overall_sql(&self) -> String {
        format!(
            "SELECT {currency} AS currency,
                    count(*) AS calls,
                    sum(total_tokens) AS tokens,
                    {cost} AS cost,
                    avg(latency_ms) AS avg_latency_ms
             FROM {AGENT_STREAM}
             WHERE _timestamp BETWEEN {} AND {}
             GROUP BY {currency}",
            self.from_micros,
            self.to_micros,
            currency = self.columns.currency(),
            cost = self.columns.cost(),
        )
    }

    /// 调用最多的模型；同一个模型用不同币种计价时分行。
    pub fn top_models_sql(&self, limit: u32) -> String {
        format!(
            "SELECT model,
                    {currency} AS currency,
                    count(*) AS calls,
                    sum(total_tokens) AS tokens,
                    {cost} AS cost
             FROM {AGENT_STREAM}
             WHERE _timestamp BETWEEN {} AND {}
             GROUP BY model, {currency}
             ORDER BY calls DESC
             LIMIT {}",
            self.from_micros,
            self.to_micros,
            limit,
            currency = self.columns.currency(),
            cost = self.columns.cost(),
        )
    }

    /// 按调用记在谁名下排行（对话的提问人、巡检的执行身份），不记在人名下的调用不算。还没有
    /// 这样的调用时没有可查的列，返回 `None`。
    pub fn top_users_sql(&self, limit: u32) -> Option<String> {
        if !self.columns.user_id {
            return None;
        }

        Some(format!(
            "SELECT user_id,
                    count(*) AS calls,
                    sum(total_tokens) AS tokens
             FROM {AGENT_STREAM}
             WHERE _timestamp BETWEEN {} AND {} AND user_id IS NOT NULL
             GROUP BY user_id
             ORDER BY calls DESC
             LIMIT {}",
            self.from_micros, self.to_micros, limit
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(columns: OptionalColumns) -> AgentStatsQuery {
        AgentStatsQuery {
            org_id: Id("a".into()),
            from_micros: 100,
            to_micros: 200,
            columns,
        }
    }

    #[test]
    fn sql_strings_include_time_range_and_limit() {
        let q = query(OptionalColumns {
            cost: true,
            user_id: true,
        });

        assert!(q.overall_sql().contains("BETWEEN 100 AND 200"));
        assert!(q.overall_sql().contains(AGENT_STREAM));
        assert!(q.overall_sql().contains("sum(cost) AS cost"));
        assert!(q.overall_sql().contains("GROUP BY cost_currency"));
        assert!(
            q.top_models_sql(5)
                .contains("GROUP BY model, cost_currency")
        );
        assert!(q.top_models_sql(5).contains("LIMIT 5"));
        assert!(q.top_users_sql(10).unwrap().contains("LIMIT 10"));
    }

    #[test]
    fn columns_never_written_are_not_referenced() {
        let q = query(OptionalColumns::from_names(["model", "latency_ms"]));

        assert!(!q.overall_sql().contains("cost_currency"));
        assert!(q.overall_sql().contains("CAST(NULL AS DOUBLE) AS cost"));
        assert!(!q.top_models_sql(5).contains("cost_currency"));
        assert_eq!(q.top_users_sql(10), None);

        assert_eq!(
            OptionalColumns::from_names(["cost", "cost_currency", "user_id"]),
            OptionalColumns {
                cost: true,
                user_id: true,
            }
        );
    }
}
