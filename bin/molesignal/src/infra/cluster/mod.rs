// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 集群基础设施扩展。
//!
//! - [`remote_clusters_repo`]：跨集群联邦查询用的 `remote_clusters` 表 CRUD。
//! - [`cluster_secrets_repo`]：`cipher_keys:<id>` token_secret_ref 的加密 token 存储。
//! - [`grpc_channel`]：跨集群 gRPC channel 构造基元（联邦查询 / 事件推送 / cancel / gossip 共用）。
//! - [`grpc_status`]：`Error` 与 gRPC `Status` 的对应关系，内部 RPC 两端共用。
//! - [`intake_forwarder`]：没有 intake 角色的节点把写入转发给 intake 节点。
//! - [`alert_forwarder`]：把外部告警转给告警来源的接收集群。
//! - [`realtime_counts`]：实时规则在本集群各节点的计数之和。
//! - [`realtime_federation`]：汇总集群是本集群的实时规则，向联邦里别的集群拉计数。

pub mod alert_forwarder;
pub mod cluster_secrets_repo;
pub mod realtime_counts;
pub mod realtime_federation;
pub use ::grpc_client as grpc_channel;
pub mod grpc_status;
pub mod intake_forwarder;
pub mod remote_clusters_repo;

pub use alert_forwarder::GrpcAlertForwarder;
pub use cluster_secrets_repo::{ClusterSecretRepository, PgClusterSecretRepository};
pub use intake_forwarder::GrpcIntakeForwarder;
pub use remote_clusters_repo::{
    PgRemoteClustersRepository, RemoteCluster, RemoteClustersRepository,
};
