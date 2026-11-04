// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The statement of a dashboard panel that draws what the explorer draws: its chart of the rows its
//! query matches, over the range the dashboard looks at. The numbers are the explorer's: the rows
//! that are stored, counted at the newest revision of a view or a session, of the sessions people
//! ran unless the query asks about the others.

use serde::{Deserialize, Serialize};

use super::selection::Selection;
use crate::{
    infra::rum::query::{
        AggregateSpec, Column, MeasureSpec, PanelColumn, Plan, distribution_sql, interval,
    },
    shared::{Error, Result},
};

/// Most groups a chart over time follows, as in the explorer; how many groups a list holds unless
/// it says, and the most it holds, as the aggregate has them.
const MAX_SERIES_GROUPS: usize = 20;
const DEFAULT_GROUPS: usize = 10;
const MAX_GROUPS: usize = 250;

/// Most rows a panel asks for: every bar of every group of a chart over time, every group of a
/// list, every bar of a distribution.
const SERIES_ROWS: usize = 10_000;
const DISTRIBUTION_ROWS: usize = 100;

/// How the explorer draws the numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Chart {
    Timeseries,
    Toplist,
    Table,
    Distribution,
}

/// What the explorer draws and what the panel calls it.
pub struct Request<'a> {
    pub selection: &'a Selection,
    pub chart: Chart,
    pub measures: &'a [MeasureSpec],
    pub group_by: &'a [String],
    /// What the panel calls the groups, in their order; a group without a name is called by its
    /// field.
    pub group_names: &'a [String],
    pub order_by: Option<&'a str>,
    pub descending: Option<bool>,
    pub limit: Option<usize>,
    /// The number a distribution is of.
    pub field: Option<&'a str>,
    /// The interval a chart over time is cut into: `${interval}` or a length.
    pub interval: Option<&'a str>,
}

/// A panel's statement, the stream it reads, and how many rows it asks for.
#[derive(Debug, Serialize)]
pub struct Panel {
    pub stream: &'static str,
    pub statement: String,
    pub columns: Vec<PanelColumn>,
    pub limit: usize,
}

pub fn panel(request: &Request<'_>) -> Result<Panel> {
    let stream = request.selection.stream;
    let conditions = request.selection.conditions()?;
    let name = |index: usize| request.group_names.get(index).map(String::as_str);

    if request.chart == Chart::Distribution {
        let field = request
            .field
            .ok_or_else(|| Error::invalid("a distribution needs a field"))?;

        let column = Column::resolve(stream, field.trim())?.ok_or_else(|| {
            Error::invalid(format!("`{field}` is not a field of {}", stream.name()))
        })?;

        let count = request
            .measures
            .first()
            .and_then(|measure| measure.name.as_deref());

        let statement = distribution_sql(stream, &conditions, &column, (name(0), count))?;

        return Ok(Panel {
            stream: stream.name(),
            statement: statement.sql,
            columns: statement.columns,
            limit: DISTRIBUTION_ROWS,
        });
    }

    let over_time = request.chart == Chart::Timeseries;

    if request.chart == Chart::Toplist && request.group_by.len() != 1 {
        return Err(Error::invalid("a top list is of one group"));
    }

    let bar = match (over_time, request.interval) {
        (true, Some(text)) => Some(interval(text)?),
        (true, None) => return Err(Error::invalid("a chart over time needs an interval")),
        (false, _) => None,
    };

    let limit = if over_time {
        request.limit.map(|limit| limit.min(MAX_SERIES_GROUPS))
    } else {
        request.limit
    };

    let plan = Plan::stored(
        stream,
        &AggregateSpec {
            measures: request.measures,
            group_by: request.group_by,
            bars: None,
            order_by: request.order_by,
            descending: request.descending,
            limit,
        },
    )?;

    let names = (0..request.group_by.len()).map(name).collect::<Vec<_>>();
    let statement = plan.panel_sql(stream, &conditions, bar, &names);

    let rows = match (over_time, request.group_by.is_empty()) {
        (true, _) => SERIES_ROWS,
        (false, true) => 1,
        (false, false) => limit.unwrap_or(DEFAULT_GROUPS).clamp(1, MAX_GROUPS),
    };

    Ok(Panel {
        stream: stream.name(),
        statement: statement.sql,
        columns: statement.columns,
        limit: rows,
    })
}

#[cfg(test)]
mod tests;
