// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent model call statistics queries.

use kernel::ids::Id;

use super::AGENT_STREAM;

/// Columns written only for some calls in the usage stream.
/// Model services with no unit price do not write `cost` or `cost_currency`; root cause analysis
/// and LLM function calls do not write `user_id`. A column that has never been written is absent
/// from the stream schema, so SQL referencing it cannot be planned. Before querying, the schema
/// determines whether to reference the column or substitute NULL.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OptionalColumns {
    pub cost: bool,
    pub user_id: bool,
}

impl OptionalColumns {
    /// Derives from field names in the stream schema.
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

    /// Total cost; NULL, not 0, if no call has ever had its cost calculated.
    fn cost(self) -> &'static str {
        if self.cost {
            "sum(cost)"
        } else {
            "CAST(NULL AS DOUBLE)"
        }
    }

    /// The cost currency: costs in different currencies are totaled in separate rows and cannot be
    /// added together.
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
    /// Totals, one row per currency; calls with no unit price go in the row with an empty currency.
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

    /// The most frequently called models; a model priced in different currencies has separate rows.
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

    /// Ranks calls by the person they are attributed to (the conversation's questioner or the
    /// inspection's execution identity), excluding calls not attributed to a person. Returns `None`
    /// if there are no such calls yet, since there is no column to query.
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
