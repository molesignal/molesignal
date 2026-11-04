// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The statements the RUM pages read with.
//!
//! The streams are ordinary Logs streams with fixed columns, so the pages ask the query
//! engine in SQL, as any reader does, and the engine's pruning, row selection and field masking
//! apply to them. This module is only the text of the statements: the filters of the explorer as
//! conditions on the columns of one stream, the relation that takes the latest revision of a view
//! or a session, and the lists, counts, facets, bars and aggregates built on both.

mod aggregate;
mod columns;
mod distribution;
#[cfg(test)]
pub(in crate::infra::rum) mod execution;
mod filter;
mod heatmap;
mod histogram;
mod member;
mod panel;
mod rows;
mod sql;
mod statements;

pub use self::{
    aggregate::{Aggregate, MeasureSpec, Plan, Spec as AggregateSpec},
    columns::{
        all_columns, default_columns, is_facetable, is_internal, requested_columns, text_columns,
        timeline_columns,
    },
    distribution::{
        BarPlan, DEFAULT_BARS, Distribution, DistributionBar, MAX_BARS, bars_sql, bounds_sql,
        read_bars, read_bounds, single_bar,
    },
    filter::{Condition, Filter, asks_about_traffic, conditions, people_only, text_conditions},
    heatmap::{
        CELLS, ClickMap, ClickedElement, ScrollBand, ScrollMap, clicks_sql, read_clicks,
        read_scroll, scroll_sql,
    },
    histogram::{Bar, BucketPlan, Histogram, Unavailable},
    member::{Column, MEMBER_COLUMNS},
    panel::{PanelColumn, PanelRole, PanelStatement, distribution_sql, interval},
    rows::{Row, rows},
    sql::{Scope, WEIGHT, WEIGHT_COLUMN, ident, literal},
    statements::{
        FACET_COUNT, FACET_VALUE, ID_COLUMN, KEY_COLUMN, KEY_COUNT, ListRequest, Seek, TS_COLUMN,
        facet_sql, histogram_sql, keys_sql, list_sql, ranked_sql, session_rows_sql,
    },
};
