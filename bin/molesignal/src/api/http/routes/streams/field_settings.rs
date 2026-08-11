// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 字段配置校验，以及系统流可变配置的最小边界。

use std::collections::HashSet;

use crate::{
    domain::stream::{Schema, StreamSettings, StreamType},
    shared::{Error, Result},
};

pub(super) fn validate_field_masking(
    settings: &StreamSettings,
    schema: &Schema,
    stream_type: StreamType,
) -> Result<()> {
    if stream_type == StreamType::Metrics && !settings.field_masking.is_empty() {
        return Err(Error::invalid(
            "metrics streams do not support field masking",
        ));
    }
    let mut fields = HashSet::new();
    for masking in &settings.field_masking {
        if !fields.insert(masking.field.as_str()) {
            return Err(Error::invalid(format!(
                "duplicate field masking override for `{}`",
                masking.field
            )));
        }
        if !schema
            .fields
            .iter()
            .any(|field| field.name == masking.field)
        {
            return Err(Error::invalid(format!(
                "unknown field masking override `{}`",
                masking.field
            )));
        }
        if let Some(algorithm) = &masking.algorithm {
            algorithm.validate()?;
        }
    }
    Ok(())
}

pub(super) fn validate_system_settings_update(
    current: &StreamSettings,
    proposed: &StreamSettings,
) -> Result<()> {
    let mut allowed = current.clone();
    allowed.index_rules = proposed.index_rules.clone();
    allowed.field_masking = proposed.field_masking.clone();
    if &allowed != proposed {
        return Err(Error::forbidden(
            "system streams only allow field indexing, extraction and masking settings",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        masking::{FieldMaskingAlgorithm, FieldMaskingOverride},
        stream::{FieldDef, FieldType, Schema, StreamSettings},
    };

    fn schema() -> Schema {
        Schema {
            fields: vec![FieldDef {
                name: "service.name".into(),
                data_type: FieldType::Utf8,
                nullable: true,
                indexed: false,
                encrypted: false,
                exact: false,
            }],
        }
    }

    #[test]
    fn stream_override_must_name_a_unique_schema_field() {
        let item = FieldMaskingOverride {
            field: "service.name".into(),
            algorithm: Some(FieldMaskingAlgorithm::default()),
        };
        let mut settings = StreamSettings {
            field_masking: vec![item.clone(), item],
            ..Default::default()
        };
        assert!(validate_field_masking(&settings, &schema(), StreamType::Logs).is_err());

        settings.field_masking[1].field = "missing".into();
        assert!(validate_field_masking(&settings, &schema(), StreamType::Logs).is_err());
    }

    #[test]
    fn metrics_reject_field_masking_overrides() {
        let settings = StreamSettings {
            field_masking: vec![FieldMaskingOverride {
                field: "service.name".into(),
                algorithm: Some(FieldMaskingAlgorithm::default()),
            }],
            ..Default::default()
        };
        assert!(validate_field_masking(&settings, &schema(), StreamType::Metrics).is_err());
    }

    #[test]
    fn system_stream_allows_only_index_and_masking_settings() {
        let current = StreamSettings::default();
        let mut proposed = current.clone();
        proposed.field_masking.push(FieldMaskingOverride {
            field: "service.name".into(),
            algorithm: None,
        });
        assert!(validate_system_settings_update(&current, &proposed).is_ok());

        proposed.description = Some("changed".into());
        assert!(validate_system_settings_update(&current, &proposed).is_err());
    }
}
