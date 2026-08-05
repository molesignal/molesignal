// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 集群基础设施扩展。
//!
//! - [`remote_clusters_repo`]：跨集群联邦查询用的 `remote_clusters` 表 CRUD。
//! - [`cluster_secrets_repo`]：`cipher_keys:<id>` token_secret_ref 的加密 token 存储。
//! - [`grpc_channel`]：跨集群 gRPC channel 构造基元（联邦查询 / 事件推送 / cancel / gossip 共用）。

pub mod cluster_secrets_repo;
pub mod grpc_channel;
pub mod remote_clusters_repo;

pub use cluster_secrets_repo::{ClusterSecretRepository, PgClusterSecretRepository};
pub use remote_clusters_repo::{
    PgRemoteClustersRepository, RemoteCluster, RemoteClustersRepository,
};
