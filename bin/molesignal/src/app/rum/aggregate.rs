// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Numbers over the rows a selection matches: for each group of up to two fields and, when asked,
//! for each bar of time. The pages of RUM and the tools of Mole Agent read through it, so what a
//! tool says is what the chart beside the list shows.

use serde_json::Value;

use super::{
    reader::{Reader, within_budget},
    selection::Selection,
};
use crate::{
    infra::rum::query::{Aggregate, AggregateSpec, BucketPlan, MeasureSpec, Plan, Scope},
    shared::{Error, Result},
};

/// What to compute over a selection.
pub struct Request<'a> {
    pub selection: &'a Selection,
    pub measures: &'a [MeasureSpec],
    pub group_by: &'a [String],
    /// Also split the answer into bars of this many microseconds.
    pub bucket_micros: Option<i64>,
    pub origin_micros: Option<i64>,
    pub order_by: Option<&'a str>,
    pub descending: Option<bool>,
    pub limit: Option<usize>,
    /// Count what the rows kept stand for (what there would be had every session been kept), not
    /// the rows that are stored.
    pub estimate: bool,
}

/// `asc` or `desc`; neither is the order the measure or the group has by default.
pub fn descending(order: Option<&str>) -> Result<Option<bool>> {
    match order.map(str::trim) {
        None | Some("") => Ok(None),
        Some(order) if order.eq_ignore_ascii_case("asc") => Ok(Some(false)),
        Some(order) if order.eq_ignore_ascii_case("desc") => Ok(Some(true)),
        Some(_) => Err(Error::invalid("order must be asc or desc")),
    }
}

/// A count out of a value of an answer: what is not a number (a value the reader may not see) is
/// none.
pub fn number(value: Option<&Value>) -> u64 {
    value
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_f64().map(|float| float.max(0.0) as u64))
        })
        .unwrap_or_default()
}

pub async fn run(reader: &Reader<'_>, request: &Request<'_>) -> Result<Aggregate> {
    let selection = request.selection;
    let stream = selection.stream;
    let span = selection.span;

    let bars = request
        .bucket_micros
        .map(|bucket| {
            BucketPlan::new(
                Some(span.from),
                Some(span.to),
                Some(bucket),
                request.origin_micros,
            )
        })
        .transpose()?;

    let spec = AggregateSpec {
        measures: request.measures,
        group_by: request.group_by,
        bars,
        order_by: request.order_by,
        descending: request.descending,
        limit: request.limit,
    };

    let plan = if request.estimate {
        Plan::new(stream, &spec)?
    } else {
        Plan::stored(stream, &spec)?
    };

    let conditions = selection.conditions()?;

    let scope = Scope {
        stream,
        from: span.from,
        to: span.to,
        conditions: &conditions,
    };

    // The groups of a series are found first and followed after, inside one budget.
    let found = within_budget(async {
        let Some(first) = reader
            .run_tracked(stream, span, plan.first_sql(&scope))
            .await?
        else {
            return Ok(None);
        };

        if !plan.needs_series() {
            return Ok(Some(first));
        }

        let groups = plan.groups_of(&first.columns, &first.rows)?;

        if groups.is_empty() {
            return Ok(None);
        }

        reader
            .run_tracked(stream, span, plan.series_sql(&scope, &groups))
            .await
    })
    .await?;

    match found {
        Ok(Some(result)) => plan.answer(&result.columns, &result.rows),
        Ok(None) => Ok(plan.empty()),
        Err(reason) => Ok(plan.unavailable(reason)),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn counts_are_read_as_numbers_and_what_is_not_one_is_none() {
        assert_eq!(number(Some(&json!(612))), 612);
        assert_eq!(number(Some(&json!(7.0))), 7);
        assert_eq!(number(Some(&json!("masked"))), 0);
        assert_eq!(number(None), 0);
    }

    #[test]
    fn an_order_is_ascending_or_descending() {
        assert_eq!(descending(None).unwrap(), None);
        assert_eq!(descending(Some(" ")).unwrap(), None);
        assert_eq!(descending(Some("ASC")).unwrap(), Some(false));
        assert_eq!(descending(Some("desc")).unwrap(), Some(true));
        assert!(descending(Some("up")).is_err());
    }
}
