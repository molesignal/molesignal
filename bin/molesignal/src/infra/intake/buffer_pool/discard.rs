// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 丢弃永远无法发布的内存数据。
//!
//! 数据集已被删除、或 writer 被更高 epoch 栅栏之后，重试 flush 不会成功。留着这些
//! generation 只会一直占着进程级内存预算，最终让所有数据集的写入都被限流。

use super::{BufferWriter, BufferedRecordBatch, RecordBuilder};

/// 被丢弃的内存数据：行数，以及要归还给进程内存预算的字节数。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Discarded {
    pub rows: usize,
    pub accounted_bytes: usize,
}

impl Discarded {
    fn add_batch(&mut self, batch: &BufferedRecordBatch) {
        self.rows += batch.batch.num_rows();
        self.accounted_bytes += batch.accounted_size_bytes();
    }

    fn add(&mut self, other: Discarded) {
        self.rows += other.rows;
        self.accounted_bytes += other.accounted_bytes;
    }
}

impl RecordBuilder {
    /// 丢弃所有尚未发布的数据：等待重试的、正在发布的和活跃的 generation。
    pub fn discard_all(&mut self) -> Discarded {
        let mut discarded = Discarded::default();

        for batch in self.pending.drain(..).chain(self.inflight.take()) {
            discarded.add_batch(&batch);
        }

        discarded.add(self.discard_active());
        discarded
    }

    /// 只丢弃属于 `writer` 的未发布数据；其它 writer（例如重启后新 epoch 的写入）不受影响。
    pub fn discard_writer(&mut self, writer: &BufferWriter) -> Discarded {
        let mut discarded = Discarded::default();

        self.pending.retain(|batch| {
            let doomed = batch.writer == *writer;

            if doomed {
                discarded.add_batch(batch);
            }

            !doomed
        });

        if self
            .inflight
            .as_ref()
            .is_some_and(|batch| batch.writer == *writer)
            && let Some(batch) = self.inflight.take()
        {
            discarded.add_batch(&batch);
        }

        if self.active_writer.as_ref() == Some(writer) {
            discarded.add(self.discard_active());
        }

        discarded
    }

    fn discard_active(&mut self) -> Discarded {
        if self.row_count == 0 {
            return Discarded::default();
        }

        let discarded = Discarded {
            rows: self.row_count,
            accounted_bytes: self.accounted_size_bytes,
        };

        for column in self.columns.values_mut() {
            let _ = column.finish();
        }

        self.active_sequences.clear();
        self.active_writer = None;
        self.accounted_size_bytes = 0;
        self.approx_size_bytes = 0;
        self.row_count = 0;
        self.active_started_at = None;
        self.collected_bytes = Some(0);
        discarded
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        domain::{
            intake::RawEvent,
            storage::{WalSequence, WriterEpoch, WriterNodeId},
            stream::{FieldDef, FieldType, Schema, StreamDefinition, StreamType},
        },
        shared::{ids::Id, time::TimestampMicros},
    };

    fn stream() -> StreamDefinition {
        StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org-a"),
            name: "app".into(),
            stream_type: StreamType::LOGS,
            schema: Schema {
                fields: vec![FieldDef {
                    name: "level".into(),
                    data_type: FieldType::Utf8,
                    nullable: false,
                    index_type: None,
                    indexed: false,
                    encrypted: false,
                    exact: false,
                }],
            },
            retention: None,
            created_at: TimestampMicros::now(),
            updated_at: TimestampMicros::now(),
        }
    }

    fn writer(epoch: u64) -> BufferWriter {
        BufferWriter::new(WriterNodeId::new("node-a"), WriterEpoch(epoch))
    }

    /// 以 `epoch` 的身份写入一行，并按 `bytes` 记账。
    fn push(builder: &mut RecordBuilder, epoch: u64, sequence: u64, bytes: usize) {
        let event = RawEvent {
            timestamp: TimestampMicros(1_000_000 + sequence as i64),
            fields: [("level".to_owned(), json!("info"))].into_iter().collect(),
        };
        builder
            .push_with_position(&event, &writer(epoch), WalSequence(sequence))
            .unwrap();
        builder.add_accounted_bytes(bytes);
    }

    /// 一个 epoch 1 的 generation（失败后等待重试，或正在发布）加一个 epoch 2 的活跃 generation。
    /// 重试 generation 与发布中的 generation 互斥：`begin_flush` 总是先取重试队列。
    fn builder_with_older_generation(inflight: bool) -> RecordBuilder {
        let mut builder = RecordBuilder::new(&stream());
        push(&mut builder, 1, 1, 10);
        let older = builder.begin_flush().unwrap().unwrap();
        if !inflight {
            builder.fail_flush(&older.flush_id()).unwrap();
        }
        push(&mut builder, 2, 2, 40);
        builder
    }

    #[test]
    fn discard_all_empties_every_generation_and_reports_the_accounting() {
        for inflight in [false, true] {
            let mut builder = builder_with_older_generation(inflight);
            assert_eq!(builder.accounted_size_bytes(), 50);

            let discarded = builder.discard_all();

            assert_eq!(
                discarded,
                Discarded {
                    rows: 2,
                    accounted_bytes: 50
                },
                "inflight={inflight}"
            );
            assert!(builder.is_empty());
            assert_eq!(builder.accounted_size_bytes(), 0);
            assert!(builder.begin_flush().unwrap().is_none());
        }
    }

    #[test]
    fn discard_writer_keeps_other_writers_and_stays_usable() {
        for inflight in [false, true] {
            let mut builder = builder_with_older_generation(inflight);

            let discarded = builder.discard_writer(&writer(1));

            assert_eq!(
                discarded,
                Discarded {
                    rows: 1,
                    accounted_bytes: 10
                },
                "inflight={inflight}"
            );
            assert_eq!(builder.row_count(), 1, "epoch 2 active row survives");
            assert_eq!(builder.accounted_size_bytes(), 40);
            let survivor = builder.begin_flush().unwrap().unwrap();
            assert_eq!(survivor.writer, writer(2));
            assert_eq!(survivor.batch.num_rows(), 1);
        }
    }

    #[test]
    fn discarded_active_generation_accepts_a_new_writer() {
        let mut builder = RecordBuilder::new(&stream());
        push(&mut builder, 1, 1, 5);

        assert_eq!(builder.discard_writer(&writer(1)).rows, 1);

        push(&mut builder, 2, 2, 7);
        let batch = builder.begin_flush().unwrap().unwrap();
        assert_eq!(batch.writer, writer(2));
        assert_eq!(batch.accounted_size_bytes(), 7);
    }

    #[test]
    fn discarding_an_empty_builder_is_a_no_op() {
        let mut builder = RecordBuilder::new(&stream());

        assert_eq!(builder.discard_all(), Discarded::default());
        assert_eq!(builder.discard_writer(&writer(1)), Discarded::default());
    }
}
