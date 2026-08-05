// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! querier 角色没有独立的启动函数：它对外提供的是 gRPC 上的 Arrow Flight 扫描分片
//! 服务（`do_get` → 读 parquet → 跑 shard SQL），由 [`super::run`] 在角色集含
//! `querier`（或 `standalone`）时统一起 [`crate::api::grpc::serve_grpc`]。
//!
//! 协调端 `DistributedDataFusionEngine` 通过 `list_role(Querier)` 发现本节点并散播分片。
