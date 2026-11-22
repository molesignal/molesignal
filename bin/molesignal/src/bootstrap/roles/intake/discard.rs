// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 永远无法发布的数据：丢出内存并归还内存预算。
//!
//! 进程内存预算由所有数据集共享。一个数据集的 buffer 若永远 flush 不出去，就会一直占着
//! 预算，最终让每个数据集的写入都被限流。

use kernel::Error;

use super::IntakeWorker;
use crate::infra::intake::{BufferKey, BufferWriter, DatasetBuffer, inc_flush_error};

impl IntakeWorker {
    /// 流或物理数据集已被删除：缓冲里的数据没有任何去处。整体丢弃并归还预算，WAL 尽力清理。
    ///
    /// buffer 本身留在池里（已空），之后的 flush 对它是空操作。
    pub(super) async fn discard_dataset_gone(
        &self,
        buffer: &DatasetBuffer,
        key: &BufferKey,
        reason: &'static str,
    ) {
        let (discarded, high_watermark_seq) = {
            let mut records = buffer.records().lock().await;
            let high_watermark_seq = records.high_watermark_seq();
            (records.discard_all(), high_watermark_seq)
        };

        self.buffer.release_memory(discarded.accounted_bytes);

        if discarded.rows == 0 {
            return;
        }

        inc_flush_error(reason);
        tracing::error!(
            dataset_id = %key,
            reason,
            rows = discarded.rows,
            "dataset no longer exists; buffered rows discarded"
        );

        // Only this epoch's objects are this process's to delete; an earlier epoch's are claimed
        // by another node, which finds the stream gone too.
        let writer_epoch = self.wal.lease().epoch();

        match self
            .cleanup_wal(key, writer_epoch, high_watermark_seq)
            .await
        {
            Ok(()) => {
                self.pending_wal_cleanup.remove(key);
            }
            Err(error) => {
                self.remember_pending_wal_cleanup(key, writer_epoch, high_watermark_seq);
                tracing::warn!(dataset_id = %key, %error, "WAL cleanup of discarded dataset deferred");
            }
        }
    }

    /// The writer epoch was fenced: its rows can no longer be published from here. They are dropped
    /// from memory and their WAL objects kept: the node that claimed the stream publishes them.
    /// Rows of other epochs (this process's new one) are untouched.
    pub(super) async fn discard_fenced_writer(
        &self,
        buffer: &DatasetBuffer,
        key: &BufferKey,
        writer: &BufferWriter,
        error: &Error,
    ) {
        let discarded = buffer.records().lock().await.discard_writer(writer);
        self.buffer.release_memory(discarded.accounted_bytes);
        inc_flush_error("writer_fenced");
        tracing::error!(
            dataset_id = %key,
            node_id = %writer.writer_node_id,
            writer_epoch = %writer.writer_epoch,
            rows = discarded.rows,
            %error,
            "writer epoch fenced; buffered rows dropped from memory, the node that claimed its \
             WAL publishes them"
        );
    }
}
