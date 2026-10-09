// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 把公共入口收到的写入交给持有写入权的节点。

use async_trait::async_trait;

use crate::{
    domain::intake::{IntakeBatch, IntakeResult},
    shared::Result,
};

/// 没有 intake 角色的节点（router、alert 等）装配它：批次原样交给 intake 节点，
/// pipeline、脱敏与 schema 演化只在 intake 节点上执行一次，应答也由 intake 节点给出。
#[async_trait]
pub trait IntakeForwarder: Send + Sync {
    async fn forward(&self, batch: IntakeBatch) -> Result<IntakeResult>;
}
