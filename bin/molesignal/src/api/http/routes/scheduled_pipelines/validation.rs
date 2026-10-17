// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::Value;

use crate::{
    domain::pipeline::{
        processing::{ProcessingKind, parse_processing_steps},
        realtime::validate_config,
        scheduled::ScheduledConfig,
    },
    infra::{pipeline::scheduled::parse_every_secs, runtime::VrlRuntime},
    shared::{Error, Result},
};

/// A pipeline as it is saved: every step compiles, and a scheduled pipeline's streams, retries
/// and dead letters are what a run can read ([`ScheduledConfig::parse`]).
pub(super) fn validate(
    source: &str,
    target: &str,
    config: &Value,
    cron: &str,
    lookback: i32,
) -> Result<()> {
    let steps = parse_processing_steps(config)?;
    let runtime = VrlRuntime::new();

    for step in &steps {
        if step.kind == ProcessingKind::Vrl {
            runtime
                .compile(&step.script)
                .map_err(|_| Error::invalid("invalid pipeline VRL script"))?;
        }
    }

    if validate_config(config, source, target)?.is_some() {
        return Ok(());
    }

    ScheduledConfig::parse(config, source, target)?;

    if parse_every_secs(cron).is_none() || lookback <= 0 {
        return Err(Error::invalid(
            "scheduled pipelines require every:Ns/m/h and a positive lookback",
        ));
    }

    Ok(())
}
