// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! querier 角色没有独立的启动函数：它对外提供的是 gRPC 上的 Arrow Flight 扫描分片
//! 服务（`do_get` → 读 parquet → 跑 shard SQL），由 [`super::run`] 在角色集含
//! `querier`（或 `standalone`）时统一起 [`crate::api::grpc::serve_grpc`]。
//!
//! 没有 querier 角色的节点用 `RemoteSqlEngine` / `RemotePromqlEngine` 经内部 gRPC
//! （`query.v1.QueryExecutionService`）把查询交给 querier 节点；querier 用本地引擎完整执行。
