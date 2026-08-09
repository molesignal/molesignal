// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 模型调用统计查询。

use super::AGENT_STREAM;
use crate::shared::ids::Id;

pub struct AgentStatsQuery {
    pub org_id: Id,
    pub from_micros: i64,
    pub to_micros: i64,
}

impl AgentStatsQuery {
    pub fn overall_sql(&self) -> String {
        format!(
            "SELECT count(*) AS calls,
                    sum(total_tokens) AS tokens,
                    sum(cost_usd) AS cost,
                    avg(latency_ms) AS avg_latency_ms
             FROM {AGENT_STREAM}
             WHERE _timestamp BETWEEN {} AND {}",
            self.from_micros, self.to_micros
        )
    }

    pub fn top_models_sql(&self, limit: u32) -> String {
        format!(
            "SELECT model,
                    count(*) AS calls,
                    sum(total_tokens) AS tokens,
                    sum(cost_usd) AS cost
             FROM {AGENT_STREAM}
             WHERE _timestamp BETWEEN {} AND {}
             GROUP BY model
             ORDER BY calls DESC
             LIMIT {}",
            self.from_micros, self.to_micros, limit
        )
    }

    pub fn top_users_sql(&self, limit: u32) -> String {
        format!(
            "SELECT user_id,
                    count(*) AS calls,
                    sum(total_tokens) AS tokens
             FROM {AGENT_STREAM}
             WHERE _timestamp BETWEEN {} AND {}
             GROUP BY user_id
             ORDER BY calls DESC
             LIMIT {}",
            self.from_micros, self.to_micros, limit
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_strings_include_time_range_and_limit() {
        let q = AgentStatsQuery {
            org_id: Id("a".into()),
            from_micros: 100,
            to_micros: 200,
        };
        assert!(q.overall_sql().contains("BETWEEN 100 AND 200"));
        assert!(q.overall_sql().contains(AGENT_STREAM));
        assert!(q.top_models_sql(5).contains("LIMIT 5"));
        assert!(q.top_users_sql(10).contains("LIMIT 10"));
    }
}
