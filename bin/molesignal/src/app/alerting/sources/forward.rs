// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 把请求交给来源的接收集群：一个来源的事件只在接收集群上开，别的集群收到它的告警原样转过去，
//! 再把接收集群的答复回给发送方。

use async_trait::async_trait;

use super::IntakeReply;
use crate::shared::{Result, ids::Id};

/// 转给接收集群的一次请求。
#[derive(Debug, Clone, Copy)]
pub struct ForwardedRequest<'a> {
    /// 来源的接收集群（集群 ID）。
    pub home_cluster: &'a str,
    /// 转发的集群，即本集群。
    pub local_cluster: &'a str,
    /// 来源在本集群里的组织；接收集群按组织映射换成它那边的组织。
    pub org_id: &'a Id,
    pub source_id: &'a Id,
    pub content_type: Option<&'a str>,
    pub body: &'a [u8],
}

#[async_trait]
pub trait AlertForwarder: Send + Sync {
    /// 交给接收集群，返回它回给发送方的答复。连不上、没有组织映射或超时时返回错误。
    async fn forward(&self, request: ForwardedRequest<'_>) -> Result<IntakeReply>;
}
