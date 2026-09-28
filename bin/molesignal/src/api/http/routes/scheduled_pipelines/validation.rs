// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::Value;

use crate::{
    domain::pipeline::realtime::validate_config,
    infra::pipeline::exec::{parse_signal_type, validate_pipeline_streams},
    shared::{Error, Result},
};

pub(super) fn validate(
    source: &str,
    target: &str,
    config: &Value,
    cron: &str,
    lookback: i32,
) -> Result<()> {
    let steps = crate::domain::pipeline::processing::parse_processing_steps(config)?;
    let runtime = crate::infra::runtime::VrlRuntime::new();
    for step in &steps {
        if step.kind == crate::domain::pipeline::processing::ProcessingKind::Vrl {
            runtime
                .compile(&step.script)
                .map_err(|_| Error::invalid("invalid pipeline VRL script"))?;
        }
    }
    if validate_config(config, source, target)?.is_some() {
        return Ok(());
    }
    if steps
        .iter()
        .any(|step| step.kind == crate::domain::pipeline::processing::ProcessingKind::Builtin)
        && !config
            .get("sources")
            .and_then(Value::as_array)
            .is_some_and(|sources| sources.iter().any(|s| s.as_str() == Some(source)))
    {
        return Err(Error::invalid(
            "built-in scheduled processing must declare its source stream",
        ));
    }
    validate_pipeline_streams(source, target, parse_signal_type(config))?;
    if crate::infra::pipeline::scheduled::parse_every_secs(cron).is_none() || lookback <= 0 {
        return Err(Error::invalid(
            "scheduled pipelines require every:Ns/m/h and a positive lookback",
        ));
    }
    Ok(())
}
