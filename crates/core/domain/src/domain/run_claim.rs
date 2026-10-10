// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 后台工作单元的运行准入。
//!
//! 多个节点分担同一批周期性后台工作时，归属（谁是 owner）只负责把重复工作压到最低：成员变化的
//! 短暂窗口里，两个节点仍可能同时认为自己是某个工作单元的 owner。会产生外部副作用的单元（发
//! 报告、跑 pipeline、拉 connector）因此在开跑前再认领一份租约：同一时刻只有一个节点持有，节点
//! 崩溃后租约到期，由别的节点重跑。

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;

use crate::shared::{Result, time::TimestampMicros};

/// 运行租约的存储。`(kind, key)` 标识一个工作单元，`kind` 区分报告、pipeline、connector 等。
#[async_trait]
pub trait RunClaimRepository: Send + Sync {
    /// 以 `holder` 的名义认领，租约持续到 `until`。
    ///
    /// 无人持有、上一份租约在 `now` 或之前到期，或租约本来就是 `holder` 的（节点重启后接着认领
    /// 自己没来得及释放的租约）时成功，返回 `true`；别人的租约仍有效时返回 `false`。
    async fn try_claim(
        &self,
        kind: &str,
        key: &str,
        holder: &str,
        now: TimestampMicros,
        until: TimestampMicros,
    ) -> Result<bool>;

    /// 释放 `holder` 持有的租约；租约已被别人接手或已不存在时什么也不做。
    async fn release(&self, kind: &str, key: &str, holder: &str) -> Result<()>;
}

/// 一个工作单元开跑前的准入判定。
#[async_trait]
pub trait RunGate: Send + Sync {
    /// `Some`：本节点可以跑这个单元，跑完要 [`RunPermit::release`]；`None`：它归别的节点，或
    /// 别的节点正在跑。
    async fn admit(&self, kind: &'static str, key: &str) -> Result<Option<RunPermit>>;
}

/// 准入凭证；持有期间租约归本节点。
pub struct RunPermit {
    held: Option<Held>,
}

struct Held {
    claims: Arc<dyn RunClaimRepository>,
    kind: &'static str,
    key: String,
    holder: String,
}

impl RunPermit {
    /// 不占用任何租约的凭证（不分担工作时使用）。
    pub fn open() -> Self {
        Self { held: None }
    }

    /// `holder` 已在 `claims` 里认领了 `(kind, key)`。
    pub fn claimed(
        claims: Arc<dyn RunClaimRepository>,
        kind: &'static str,
        key: impl Into<String>,
        holder: impl Into<String>,
    ) -> Self {
        Self {
            held: Some(Held {
                claims,
                kind,
                key: key.into(),
                holder: holder.into(),
            }),
        }
    }

    /// 跑完后释放租约，下一个周期才能再被认领。释放失败不影响已完成的工作：租约到期后同样会
    /// 失效，只是下一轮要多等一会儿。
    pub async fn release(self) -> Result<()> {
        match self.held {
            Some(held) => {
                held.claims
                    .release(held.kind, &held.key, &held.holder)
                    .await
            }
            None => Ok(()),
        }
    }
}

/// 不分担工作：每个单元都准入，也不认领租约。
pub struct OpenGate;

#[async_trait]
impl RunGate for OpenGate {
    async fn admit(&self, _kind: &'static str, _key: &str) -> Result<Option<RunPermit>> {
        Ok(Some(RunPermit::open()))
    }
}

/// 进程内的租约存储，供测试与单进程使用。
#[derive(Default)]
pub struct MemoryRunClaims {
    leases: Mutex<HashMap<(String, String), Lease>>,
}

struct Lease {
    holder: String,
    until: TimestampMicros,
}

#[async_trait]
impl RunClaimRepository for MemoryRunClaims {
    async fn try_claim(
        &self,
        kind: &str,
        key: &str,
        holder: &str,
        now: TimestampMicros,
        until: TimestampMicros,
    ) -> Result<bool> {
        let mut leases = self.leases.lock().unwrap_or_else(|e| e.into_inner());
        let slot = (kind.to_owned(), key.to_owned());

        let free = leases
            .get(&slot)
            .is_none_or(|lease| lease.until.0 <= now.0 || lease.holder == holder);

        if free {
            leases.insert(
                slot,
                Lease {
                    holder: holder.to_owned(),
                    until,
                },
            );
        }

        Ok(free)
    }

    async fn release(&self, kind: &str, key: &str, holder: &str) -> Result<()> {
        let mut leases = self.leases.lock().unwrap_or_else(|e| e.into_inner());
        let slot = (kind.to_owned(), key.to_owned());

        if leases
            .get(&slot)
            .is_some_and(|lease| lease.holder == holder)
        {
            leases.remove(&slot);
        }

        Ok(())
    }
}
