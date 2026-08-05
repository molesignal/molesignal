// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Traces 聚合：service graph edges 等派生数据。
//!
//! - [`service_graph::ServiceGraphAggregator`]：DashMap 分钟桶聚合器，按
//!   `(org, client, server, bucket_minute)` 累计 request_count / error_count /
//!   p50/p95/p99 us；满桶后由 ingester flush 触发 [`flush_one`] 落 DB。
//! - [`service_graph::ServiceGraphRepository`]：service_graph_edges 表的 trait + Pg 实装。

pub mod service_graph;
pub mod summary_reader;

pub use service_graph::{
    EdgeKey, EdgeSnapshot, PgServiceGraphRepository, ServiceGraphAggregator,
    ServiceGraphObserverImpl, ServiceGraphRepository,
};
