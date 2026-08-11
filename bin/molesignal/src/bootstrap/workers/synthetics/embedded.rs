// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! In-process Probe execution for the built-in Local Location.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context as _, Result};
use prost::Message as _;
use tokio::{sync::Semaphore, task::JoinHandle};

use crate::{
    api::grpc::probe::{result::result_from_wire, task::task_to_wire},
    app::synthetics::SyntheticService,
    domain::synthetics::{ProbeAgent, ProbeTask},
    protocol::probe::v1 as wire,
    shared::time::TimestampMicros,
};

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const LEASE_RENEWAL_INTERVAL: Duration = Duration::from_secs(30);
const LEASE_RENEWAL_WINDOW_MICROS: i64 = 120 * 1_000_000;
const MAX_EMBEDDED_CONCURRENCY: u32 = 32;

pub struct EmbeddedProbeRunner {
    synthetics: Arc<SyntheticService>,
    agent: ProbeAgent,
    max_concurrent: usize,
}

impl EmbeddedProbeRunner {
    pub fn new(synthetics: Arc<SyntheticService>, agent: ProbeAgent) -> Self {
        let max_concurrent = agent
            .capacity
            .max_concurrent
            .clamp(1, MAX_EMBEDDED_CONCURRENCY) as usize;
        Self {
            synthetics,
            agent,
            max_concurrent,
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            tracing::info!(
                agent_id = %self.agent.id,
                location_id = %self.agent.location_id,
                max_concurrent = self.max_concurrent,
                "Standalone embedded Probe runner started"
            );
            self.run().await;
        })
    }

    async fn run(self) {
        let permits = Arc::new(Semaphore::new(self.max_concurrent));
        let sequence = Arc::new(AtomicU64::new(self.agent.last_result_sequence));
        let mut tick = tokio::time::interval(POLL_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            while permits.available_permits() > 0 {
                let permit = match permits.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => break,
                };
                let leased = self.synthetics.lease_next_probe_task(&self.agent).await;
                let (task, lease_token) = match leased {
                    Ok(Some(leased)) => leased,
                    Ok(None) => {
                        drop(permit);
                        break;
                    }
                    Err(error) => {
                        drop(permit);
                        tracing::warn!(%error, "embedded Probe task lease failed");
                        break;
                    }
                };
                let task_id = task.id.clone();
                let synthetics = self.synthetics.clone();
                let agent = self.agent.clone();
                let sequence = sequence.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    if let Err(error) =
                        execute_task(synthetics, agent, task, lease_token, sequence).await
                    {
                        tracing::warn!(
                            task_id = %task_id,
                            error = %format_args!("{error:#}"),
                            "embedded Probe task failed"
                        );
                    }
                });
            }
        }
    }
}

#[tracing::instrument(
    name = "worker.synthetics_embedded_probe",
    parent = None,
    skip_all,
    fields(
        otel.kind = "internal",
        molesignal.worker.name = "synthetics_embedded_probe",
        molesignal.probe.agent_id = %agent.id,
        molesignal.probe.location_id = %agent.location_id,
        molesignal.probe.task_id = %task.id,
        molesignal.probe.monitor_id = %task.monitor_id,
    )
)]
async fn execute_task(
    synthetics: Arc<SyntheticService>,
    agent: ProbeAgent,
    task: ProbeTask,
    lease_token: String,
    sequence: Arc<AtomicU64>,
) -> Result<()> {
    let prepared = async {
        let location = synthetics.probe_task_location(&task).await?;
        let secrets = synthetics.resolve_probe_task_secrets(&task).await?;
        task_to_wire(task.clone(), lease_token.clone(), location, secrets)
    }
    .await;
    let wire_task = match prepared {
        Ok(task) => task,
        Err(error) => {
            if let Err(reject_error) = synthetics
                .acknowledge_probe_task(&agent, &task.id, &lease_token, false)
                .await
            {
                tracing::warn!(
                    task_id = %task.id,
                    error = %reject_error,
                    "embedded Probe task rejection failed"
                );
            }
            return Err(anyhow::anyhow!(error.to_string())).context("prepare embedded Probe task");
        }
    };
    synthetics
        .acknowledge_probe_task(&agent, &task.id, &lease_token, true)
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))
        .context("acknowledge embedded Probe task")?;

    let result_sequence = sequence.fetch_add(1, Ordering::SeqCst).saturating_add(1);
    let result = execute_with_lease_renewal(
        synthetics.clone(),
        &agent,
        &task,
        &lease_token,
        wire_task,
        result_sequence,
    )
    .await?;
    let result = result_from_wire(&agent, &task, result)
        .map_err(|error| anyhow::anyhow!(error.to_string()))
        .context("decode embedded Probe result")?;
    let processed = synthetics
        .process_result(result)
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))
        .context("persist embedded Probe result")?;
    tracing::debug!(
        task_id = %task.id,
        monitor_id = %task.monitor_id,
        outcome = ?processed.result.outcome,
        historical_only = processed.historical_only,
        "embedded Probe task completed"
    );
    Ok(())
}

async fn execute_with_lease_renewal(
    synthetics: Arc<SyntheticService>,
    agent: &ProbeAgent,
    task: &ProbeTask,
    lease_token: &str,
    wire_task: wire::ProbeTask,
    result_sequence: u64,
) -> Result<wire::ProbeResult> {
    let mut execution = Box::pin(execute_wire_task(wire_task, result_sequence));
    let mut renewal = tokio::time::interval(LEASE_RENEWAL_INTERVAL);
    renewal.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    renewal.tick().await;
    loop {
        tokio::select! {
            result = &mut execution => return result,
            _ = renewal.tick() => {
                let now = TimestampMicros::now();
                let requested_until = TimestampMicros(
                    now.0.saturating_add(LEASE_RENEWAL_WINDOW_MICROS),
                );
                if let Err(error) = synthetics
                    .renew_probe_task_lease(agent, &task.id, lease_token, requested_until)
                    .await
                {
                    tracing::warn!(
                        task_id = %task.id,
                        %error,
                        "embedded Probe task lease renewal failed"
                    );
                }
            }
        }
    }
}

async fn execute_wire_task(
    task: wire::ProbeTask,
    result_sequence: u64,
) -> Result<wire::ProbeResult> {
    let encoded = task.encode_to_vec();
    let agent_task = probe_agent::protocol::v1::ProbeTask::decode(encoded.as_slice())
        .context("bridge embedded task into Probe executor protocol")?;
    let result = probe_agent::executor::execute(agent_task, result_sequence)
        .await
        .context("execute embedded Probe task")?;
    wire::ProbeResult::decode(result.encode_to_vec().as_slice())
        .context("bridge embedded result into control-plane protocol")
}
