// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The statement of a dashboard panel that draws what a plan answers: the same measures of the
//! same rows, in the order and the number of groups the plan says, over the range the dashboard
//! looks at.
//!
//! A chart over time follows the groups that rank first over the whole range, as the explorer
//! does; the explorer finds them with one statement and follows them with a second, a panel does
//! both in one. Its answer is long: a row per bar and group, which the dashboard draws as a line
//! per group.

use super::plan::{Order, Plan};
use crate::{
    domain::rum::event::RumStream,
    infra::rum::query::{
        filter::Condition,
        panel::{Names, PanelColumn, PanelRole, PanelStatement, relation, shown, where_clause},
        sql::ident,
    },
};

fn direction(descending: bool) -> &'static str {
    if descending { "DESC" } else { "ASC" }
}

impl Plan {
    /// The statement of a panel. With an `interval`, the answer is over time, a bar of that
    /// interval at a time. `names` are what the panel calls the groups, in order: a group without
    /// one is called by its field, and a measure is called by its own name.
    pub fn panel_sql(
        &self,
        stream: RumStream,
        conditions: &[Condition],
        interval: Option<&str>,
        names: &[Option<&str>],
    ) -> PanelStatement {
        // The time, when there is one, is called `time`: no other column may be.
        let mut taken = Names::default();
        let mut asked = names.iter().copied();
        let mut columns = Vec::new();

        if interval.is_some() {
            columns.push(PanelColumn {
                name: "time".into(),
                role: PanelRole::Time,
                unit: "none",
            });
        }

        for group in &self.groups {
            columns.push(PanelColumn {
                name: taken.name(asked.next().flatten(), group.name()),
                role: PanelRole::Group,
                unit: "none",
            });
        }

        let measures = self
            .measures
            .iter()
            .map(|measure| {
                let (sql, unit) = shown(&measure.sql, measure.unit);
                let name = taken.name(asked.next().flatten(), &measure.name);

                columns.push(PanelColumn {
                    name: name.clone(),
                    role: PanelRole::Measure,
                    unit,
                });

                format!("{sql} AS {}", ident(&name))
            })
            .collect::<Vec<_>>();

        let groups = self
            .groups
            .iter()
            .zip(
                columns
                    .iter()
                    .filter(|column| column.role == PanelRole::Group),
            )
            .map(|(group, column)| format!("{} AS {}", group.text(), ident(&column.name)))
            .collect::<Vec<_>>();

        let sql = match interval {
            Some(interval) => self.series(stream, conditions, interval, &groups, &measures),
            None => self.ranked(stream, conditions, &groups, &measures),
        };

        PanelStatement { sql, columns }
    }

    /// A row per group, the first of them ranking first; a single row when nothing is grouped.
    fn ranked(
        &self,
        stream: RumStream,
        conditions: &[Condition],
        groups: &[String],
        measures: &[String],
    ) -> String {
        let select = groups.iter().chain(measures).cloned().collect::<Vec<_>>();

        let head = format!(
            "SELECT {} FROM {}{}",
            select.join(", "),
            relation(stream),
            where_clause(conditions, &[])
        );

        if groups.is_empty() {
            return head;
        }

        let first = match self.order {
            Order::Measure { index, descending } => {
                Some((groups.len() + index + 1, direction(descending)))
            }
            Order::Group { index, descending } => Some((index + 1, direction(descending))),
            Order::Bar { .. } => None,
        };

        let mut keys = first
            .map(|(ordinal, direction)| format!("{ordinal} {direction} NULLS LAST"))
            .into_iter()
            .collect::<Vec<_>>();

        // Groups that tie come in the order of their values, as the explorer lists them.
        for ordinal in 1..=groups.len() {
            let key = format!("{ordinal} ASC NULLS LAST");

            if !keys
                .iter()
                .any(|taken| taken.starts_with(&format!("{ordinal} ")))
            {
                keys.push(key);
            }
        }

        format!(
            "{head} GROUP BY {} ORDER BY {} LIMIT {}",
            ordinals(groups.len()),
            keys.join(", "),
            self.limit
        )
    }

    /// A row per bar of the interval and group. The groups are the ones the first measure ranks
    /// first over the whole range.
    fn series(
        &self,
        stream: RumStream,
        conditions: &[Condition],
        interval: &str,
        groups: &[String],
        measures: &[String],
    ) -> String {
        let bar = format!("date_bin(INTERVAL '{interval}', _timestamp) AS \"time\"");

        let select = std::iter::once(bar)
            .chain(groups.iter().cloned())
            .chain(measures.iter().cloned())
            .collect::<Vec<_>>();

        let keys = ordinals(1 + groups.len());

        if groups.is_empty() {
            return format!(
                "SELECT {} FROM {}{} GROUP BY 1 ORDER BY 1",
                select.join(", "),
                relation(stream),
                where_clause(conditions, &[])
            );
        }

        let ranked = self
            .groups
            .iter()
            .enumerate()
            .map(|(index, group)| format!("{} AS \"__g{index}\"", group.text()))
            .chain(
                self.measures
                    .first()
                    .map(|measure| format!("{} AS \"__rank\"", measure.sql)),
            )
            .collect::<Vec<_>>();

        let ties = (1..=self.groups.len())
            .map(|ordinal| format!(", {ordinal} ASC NULLS LAST"))
            .collect::<String>();

        // A group without a value is one too, as in the explorer: it is matched as such.
        let joined = self
            .groups
            .iter()
            .enumerate()
            .map(|(index, group)| {
                format!(
                    "({} IS NOT DISTINCT FROM \"__top\".\"__g{index}\")",
                    group.text()
                )
            })
            .collect::<Vec<_>>()
            .join(" AND ");

        format!(
            "WITH \"__top\" AS (SELECT {} FROM {}{} GROUP BY {} ORDER BY {} DESC NULLS LAST{ties} LIMIT {}) \
             SELECT {} FROM {} JOIN \"__top\" ON {joined}{} GROUP BY {keys} ORDER BY {keys}",
            ranked.join(", "),
            relation(stream),
            where_clause(conditions, &[]),
            ordinals(self.groups.len()),
            self.groups.len() + 1,
            self.limit,
            select.join(", "),
            relation(stream),
            where_clause(conditions, &[]),
        )
    }
}

/// `1, 2, 3`: the first `count` columns of a statement.
fn ordinals(count: usize) -> String {
    (1..=count)
        .map(|ordinal| ordinal.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}
