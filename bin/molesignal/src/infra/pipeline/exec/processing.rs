// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Ordered processing with destination metadata separate from the event payload.
use serde_json::Value;

use crate::{
    domain::pipeline::processing::{ProcessingKind, parse_processing_steps},
    infra::runtime::VrlRuntime,
    shared::{Error, Result},
};

pub struct ProcessedRows {
    pub rows: Vec<(String, Value)>,
    pub errors: Vec<String>,
}

pub fn process_rows(
    runtime: &VrlRuntime,
    config: &Value,
    fallback_target: &str,
    events: Vec<Value>,
) -> Result<ProcessedRows> {
    let steps = parse_processing_steps(config)?;
    let programs = steps
        .iter()
        .map(|step| {
            if step.kind == ProcessingKind::Vrl {
                runtime.compile(&step.script).map(Some).map_err(|e| {
                    Error::invalid(format!("step `{}` compile: {e}", step.transform_name))
                })
            } else {
                Ok(None)
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for mut event in events {
        let mut destination = fallback_target.to_owned();
        let mut failed = false;
        for (step, program) in steps.iter().zip(&programs) {
            if let Some(program) = program {
                if let Err(error) = runtime.run(program, &mut event) {
                    errors.push(format!("step `{}` run: {error}", step.transform_name));
                    failed = true;
                    break;
                }
            } else {
                destination = step.routing.destination(&event, &step.target);
            }
        }
        if !failed {
            if config
                .get("sources")
                .and_then(Value::as_array)
                .is_some_and(|sources| sources.iter().any(|s| s.as_str() == Some(&destination)))
            {
                return Err(Error::invalid(
                    "scheduled routing cannot write to a source stream",
                ));
            }
            rows.push((destination, event));
        }
    }
    Ok(ProcessedRows { rows, errors })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn builtin_routes_at_its_position_between_vrl_steps() {
        let config = json!({"steps":[
            {"script":".appname = \"orders\""},
            {"kind":"builtin","operation":"route","routing":{"kind":"field","field":"appname","prefix":"logs_","fallback":"default"}},
            {"script":".appname = \"changed\""}
        ]});
        let result =
            process_rows(&VrlRuntime::new(), &config, "fallback", vec![json!({})]).unwrap();
        assert_eq!(
            result.rows,
            vec![("logs_orders".into(), json!({"appname":"changed"}))]
        );
        assert!(result.errors.is_empty());
    }

    #[test]
    fn fixed_route_and_invalid_field_fallback_are_supported() {
        for (routing, target) in [
            (json!({"kind":"fixed"}), "orders"),
            (
                json!({"kind":"field","field":"appname","fallback":"default"}),
                "default",
            ),
        ] {
            let config = json!({"steps":[{"kind":"builtin","operation":"route","target":"orders","routing":routing}]});
            let result = process_rows(
                &VrlRuntime::new(),
                &config,
                "unused",
                vec![json!({"appname":42})],
            )
            .unwrap();
            assert_eq!(result.rows[0].0, target);
        }
    }

    #[test]
    fn unknown_operations_and_scheduled_self_routes_are_rejected() {
        let config = json!({"steps":[{"kind":"builtin","operation":"unknown"}]});
        assert!(process_rows(&VrlRuntime::new(), &config, "target", vec![]).is_err());
        let config = json!({"sources":["default"],"steps":[{"kind":"builtin","operation":"route","target":"default"}]});
        assert!(process_rows(&VrlRuntime::new(), &config, "target", vec![json!({})]).is_err());
    }
}
