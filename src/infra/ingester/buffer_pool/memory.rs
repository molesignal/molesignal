// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crate::{
    domain::stream::StreamType,
    infra::ingester::metrics::{add_reserved_buffer_bytes, inc_memory_rejection},
    shared::{Error, Result},
};

/// 进程级 buffer 内存预算。计费单位使用序列化后的原始 WAL payload 字节。
pub(super) struct MemoryBudget {
    max_bytes: usize,
    reserved_bytes: AtomicUsize,
}

impl MemoryBudget {
    pub(super) fn new(max_bytes: usize) -> Arc<Self> {
        Arc::new(Self {
            max_bytes,
            reserved_bytes: AtomicUsize::new(0),
        })
    }

    pub(super) fn try_reserve(
        self: &Arc<Self>,
        stream_type: StreamType,
        bytes: usize,
    ) -> Result<MemoryReservation> {
        if !self.reserve_with_limit(bytes) {
            inc_memory_rejection(stream_type.as_str());
            return Err(Error::resource_exhausted(
                "ingester buffer memory limit exceeded; retry later",
            ));
        }
        Ok(MemoryReservation::new(self.clone(), bytes))
    }

    /// WAL replay 已经代表 durable 数据，必须恢复进内存并显式计费，即使暂时超过当前 cap。
    pub(super) fn force_reserve(self: &Arc<Self>, bytes: usize) -> Result<MemoryReservation> {
        self.reserved_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(bytes)
            })
            .map_err(|_| Error::internal("ingester buffer memory accounting overflow"))?;
        add_reserved_buffer_bytes(bytes_as_i64(bytes));
        Ok(MemoryReservation::new(self.clone(), bytes))
    }

    pub(super) fn release(&self, bytes: usize) {
        if bytes == 0 {
            return;
        }
        let result =
            self.reserved_bytes
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                    current.checked_sub(bytes)
                });
        debug_assert!(result.is_ok(), "buffer memory accounting underflow");
        if result.is_ok() {
            add_reserved_buffer_bytes(-bytes_as_i64(bytes));
        }
    }

    pub(super) fn reserved_bytes(&self) -> usize {
        self.reserved_bytes.load(Ordering::Acquire)
    }

    fn reserve_with_limit(&self, bytes: usize) -> bool {
        let reserved = self
            .reserved_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current
                    .checked_add(bytes)
                    .filter(|next| *next <= self.max_bytes)
            })
            .is_ok();
        if reserved {
            add_reserved_buffer_bytes(bytes_as_i64(bytes));
        }
        reserved
    }
}

impl Drop for MemoryBudget {
    fn drop(&mut self) {
        let remaining = self.reserved_bytes.load(Ordering::Acquire);
        if remaining > 0 {
            add_reserved_buffer_bytes(-bytes_as_i64(remaining));
        }
    }
}

/// Reservation 在 WAL append 前创建；失败自动释放，成功后 `commit` 转成交由 builder 管理的字节数。
pub struct MemoryReservation {
    budget: Arc<MemoryBudget>,
    bytes: usize,
    committed: bool,
}

impl MemoryReservation {
    fn new(budget: Arc<MemoryBudget>, bytes: usize) -> Self {
        Self {
            budget,
            bytes,
            committed: false,
        }
    }

    pub fn commit(mut self) -> usize {
        self.committed = true;
        self.bytes
    }
}

impl Drop for MemoryReservation {
    fn drop(&mut self) {
        if !self.committed {
            self.budget.release(self.bytes);
        }
    }
}

fn bytes_as_i64(bytes: usize) -> i64 {
    i64::try_from(bytes).unwrap_or(i64::MAX)
}
