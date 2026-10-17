// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Delivering a run's events to the pipeline's connectors. A delivery that fails is tried again
//! within the run, waiting twice as long each time, as often as the pipeline's retry policy says.
//! What still does not go out becomes dead letters: a later run never delivers it again, so a
//! retried window cannot deliver the same events twice.

use std::time::Duration;

use serde_json::Value;

use super::{DeadLetter, Ledger, PipelineOutput, PipelineSpec};
use crate::{
    infra::{connectors::Connector, pipeline::exec::processing::ProcessedEvent},
    shared::{Error, ids::Id},
};

/// The longest wait between two attempts at a delivery.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// The key under which a source's rows count as delivered to a connector.
pub(super) fn connector_key(id: &str) -> String {
    format!("connector:{id}")
}

pub(super) async fn deliver(
    output: &PipelineOutput,
    pipeline: &PipelineSpec<'_>,
    rows: &[ProcessedEvent],
    ledger: &mut Ledger,
) {
    for id in &pipeline.config.connectors {
        let key = connector_key(id);

        let indices: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| !ledger.sources[row.source].done.contains(&key))
            .map(|(index, _)| index)
            .collect();

        if !indices.is_empty() {
            let events: Vec<Value> = indices
                .iter()
                .map(|&index| rows[index].event.clone())
                .collect();

            let (label, delivered) = match output
                .connectors
                .get(pipeline.org_id, &Id(id.clone()))
                .await
            {
                Ok(connector) => {
                    let retries = pipeline.config.retry.delivery_retries;
                    let delivered = dispatch(output, &connector, &events, retries).await;

                    (connector.name, delivered)
                }
                Err(e) => (id.clone(), Err((1, e))),
            };

            if let Err((attempts, e)) = delivered {
                ledger.errors.push(format!(
                    "connector `{label}` did not take {} event(s) after {attempts} attempt(s): {e}",
                    events.len()
                ));

                let reason = e.to_string();

                for (&index, event) in indices.iter().zip(events) {
                    let source = ledger.names[rows[index].source].clone();
                    ledger.failed[index] = true;
                    ledger.dead_letter(|| DeadLetter::undelivered(source, event, &label, &reason));
                }
            }
        }

        for source in &mut ledger.sources {
            source.done.insert(key.clone());
        }
    }
}

/// Delivers `events`, trying `retries` more times after a failure. A failure gives the number of
/// attempts made and the last error.
async fn dispatch(
    output: &PipelineOutput,
    connector: &Connector,
    events: &[Value],
    retries: u32,
) -> Result<(), (u32, Error)> {
    let mut wait = output.delivery_backoff;
    let mut attempt = 1;

    loop {
        match output.dispatcher.dispatch(connector, events).await {
            Ok(()) => return Ok(()),
            Err(e) if attempt > retries => return Err((attempt, e)),
            Err(e) => {
                tracing::debug!(
                    connector = %connector.id.0,
                    attempt,
                    error = %e,
                    "pipeline delivery failed; trying again"
                );

                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(MAX_BACKOFF);
                attempt += 1;
            }
        }
    }
}
