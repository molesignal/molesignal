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

use super::super::polling::PollingBackoff;
use crate::{
    api::grpc::probe::{result::result_from_wire, task::task_to_wire},
    app::synthetics::SyntheticService,
    domain::synthetics::{MonitorSpec, ProbeAgent, ProbeCapability, ProbeTask},
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
    max_browser_concurrent: usize,
    artifact_base_url: String,
}

impl EmbeddedProbeRunner {
    pub fn new(
        synthetics: Arc<SyntheticService>,
        agent: ProbeAgent,
        artifact_base_url: String,
    ) -> Self {
        let max_concurrent = agent
            .capacity
            .max_concurrent
            .clamp(1, MAX_EMBEDDED_CONCURRENCY) as usize;
        let max_browser_concurrent =
            (agent.capacity.max_browser_concurrent.min(4) as usize).min(max_concurrent);
        Self {
            synthetics,
            agent,
            max_concurrent,
            max_browser_concurrent,
            artifact_base_url,
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
        let browser_permits = Arc::new(Semaphore::new(self.max_browser_concurrent));
        let sequence = Arc::new(AtomicU64::new(self.agent.last_result_sequence));
        let mut backoff = PollingBackoff::new(
            POLL_INTERVAL,
            Duration::from_secs(10),
            "synthetics-embedded-probe",
        );
        loop {
            let mut leased_any = false;
            while permits.available_permits() > 0 {
                let permit = match permits.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => break,
                };
                let mut lease_agent = self.agent.clone();
                if browser_permits.available_permits() == 0 {
                    lease_agent
                        .capabilities
                        .retain(|capability| capability != &ProbeCapability::Browser);
                }
                let leased = self.synthetics.lease_next_probe_task(&lease_agent).await;
                let (task, lease_token) = match leased {
                    Ok(Some(leased)) => {
                        leased_any = true;
                        leased
                    }
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
                let browser_permit = if matches!(&task.spec, MonitorSpec::Browser(_)) {
                    match browser_permits.clone().try_acquire_owned() {
                        Ok(permit) => Some(permit),
                        Err(_) => {
                            drop(permit);
                            if let Err(error) = self
                                .synthetics
                                .acknowledge_probe_task(&self.agent, &task.id, &lease_token, false)
                                .await
                            {
                                tracing::warn!(
                                    task_id = %task.id,
                                    %error,
                                    "embedded Browser task capacity rejection failed"
                                );
                            }
                            continue;
                        }
                    }
                } else {
                    None
                };
                let task_id = task.id.clone();
                let synthetics = self.synthetics.clone();
                let agent = self.agent.clone();
                let sequence = sequence.clone();
                let artifact_base_url = self.artifact_base_url.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let _browser_permit = browser_permit;
                    if let Err(error) = execute_task(
                        synthetics,
                        agent,
                        task,
                        lease_token,
                        sequence,
                        artifact_base_url,
                    )
                    .await
                    {
                        tracing::warn!(
                            task_id = %task_id,
                            error = %format_args!("{error:#}"),
                            "embedded Probe task failed"
                        );
                    }
                });
            }

            if permits.available_permits() == 0 {
                if let Ok(permit) = permits.clone().acquire_owned().await {
                    drop(permit);
                }
                backoff.reset();
                continue;
            }
            if leased_any {
                backoff.reset();
            }
            tokio::time::sleep(backoff.next_delay()).await;
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
    artifact_base_url: String,
) -> Result<()> {
    let prepared = async {
        let location = synthetics.probe_task_location(&task).await?;
        let secrets = synthetics.resolve_probe_task_secrets(&task).await?;
        task_to_wire(
            task.clone(),
            lease_token.clone(),
            location,
            secrets,
            Some(&artifact_base_url),
        )
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
