// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 把排队的 Mole Agent 模型调用记录成批写进 `_agent_model_traces`。
//!
//! 每个节点一个，消费本节点 [`crate::app::agent_usage::TraceQueue`] 的另一端。一批最多
//! 取当时已经排着的 64 条，按组织分开写；写不进去只记日志，用量计数在 Postgres 里不受影响。

use std::{collections::HashMap, sync::Arc};

use tokio::task::JoinHandle;

use crate::{
    agent::telemetry::AGENT_STREAM,
    app::{
        agent_usage::{TraceEvent, TraceReceiver},
        intake::IntakeService,
    },
    domain::{
        intake::{IntakeBatch, RawEvent},
        stream::StreamType,
    },
    shared::{ids::Id, time::TimestampMicros},
};

const BATCH: usize = 64;

pub struct AgentTraceWriter {
    receiver: TraceReceiver,
    intake: Arc<IntakeService>,
}

impl AgentTraceWriter {
    pub fn new(receiver: TraceReceiver, intake: Arc<IntakeService>) -> Self {
        Self { receiver, intake }
    }

    pub fn spawn(mut self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut pending = Vec::with_capacity(BATCH);

            while let Some(first) = self.receiver.recv().await {
                pending.push(first);

                while pending.len() < BATCH {
                    match self.receiver.try_recv() {
                        Ok(event) => pending.push(event),
                        Err(_) => break,
                    }
                }

                self.flush(std::mem::take(&mut pending)).await;
            }
        })
    }

    #[tracing::instrument(
        name = "worker.agent_traces",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "agent_traces")
    )]
    async fn flush(&self, events: Vec<TraceEvent>) {
        let mut by_org: HashMap<Id, Vec<RawEvent>> = HashMap::new();

        for event in events {
            by_org.entry(event.org_id).or_default().push(RawEvent {
                timestamp: event.at,
                fields: event.fields,
            });
        }

        for (org_id, events) in by_org {
            let written = self
                .intake
                .intake_internal_telemetry(IntakeBatch {
                    batch_id: Id::new(),
                    org_id: org_id.clone(),
                    stream: AGENT_STREAM.into(),
                    stream_type: StreamType::LOGS,
                    events,
                    received_at: TimestampMicros::now(),
                })
                .await;

            if let Err(error) = written {
                tracing::debug!(org = %org_id.0, %error, "agent model traces were not written");
            }
        }
    }
}
