// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Extend-table field schema validation and transition rules.

use std::collections::HashSet;

use serde::Deserialize;

use super::normalize_field;
use crate::{
    infra::pipeline::{ExtendTableSummary, ExtendValueField},
    shared::{Error, Result},
};

#[derive(Debug, Deserialize)]
pub(super) struct UpdateTableFieldsReq {
    #[serde(default)]
    pub(super) value_fields: Vec<ExtendValueField>,
}

#[derive(Debug)]
pub(super) struct ValidatedFieldUpdate {
    pub(super) value_fields: Vec<ExtendValueField>,
    pub(super) require_empty: bool,
}

pub(super) fn validate_field_update(
    existing: &ExtendTableSummary,
    fields: Vec<ExtendValueField>,
) -> Result<ValidatedFieldUpdate> {
    if fields.len() > 100 {
        return Err(Error::invalid("value_fields cannot exceed 100 fields"));
    }

    let mut seen = HashSet::new();
    let value_fields = fields
        .into_iter()
        .map(|field| normalize_field(field, &mut seen))
        .collect::<Result<Vec<_>>>()?;

    let mut require_empty = false;
    for current in &existing.value_fields {
        let Some(next) = value_fields
            .iter()
            .find(|candidate| candidate.name == current.name)
        else {
            require_empty = true;
            if existing.row_count > 0 {
                return Err(Error::conflict(format!(
                    "field {} cannot be removed or renamed while the table contains records",
                    current.name
                )));
            }
            continue;
        };

        if next.field_type != current.field_type {
            require_empty = true;
            if existing.row_count > 0 {
                return Err(Error::conflict(format!(
                    "field {} type cannot change while the table contains records",
                    current.name
                )));
            }
        }
    }

    Ok(ValidatedFieldUpdate {
        value_fields,
        require_empty,
    })
}

#[cfg(test)]
mod tests {
    use super::validate_field_update;
    use crate::{
        infra::pipeline::{ExtendTableSummary, ExtendValueField},
        shared::{Error, time::TimestampMicros},
    };

    fn field(name: &str, field_type: &str, required: bool, description: &str) -> ExtendValueField {
        ExtendValueField {
            name: name.to_string(),
            field_type: field_type.to_string(),
            required,
            description: description.to_string(),
        }
    }

    fn table(row_count: i64) -> ExtendTableSummary {
        ExtendTableSummary {
            table_name: "customers".to_string(),
            description: String::new(),
            key_field: "customer_id".to_string(),
            value_fields: vec![field("tier", "string", false, "Old description")],
            row_count,
            updated_at: TimestampMicros(0),
        }
    }

    #[test]
    fn populated_table_allows_metadata_changes_and_new_fields() {
        let update = validate_field_update(
            &table(2),
            vec![
                field("tier", "string", true, "Customer tier"),
                field("region", "string", false, "Region"),
            ],
        )
        .expect("safe field update");

        assert!(!update.require_empty);
        assert_eq!(update.value_fields.len(), 2);
        assert!(update.value_fields[0].required);
    }

    #[test]
    fn populated_table_rejects_field_removal_or_rename() {
        let error = validate_field_update(
            &table(1),
            vec![field("plan", "string", false, "Renamed field")],
        )
        .expect_err("renaming removes the stored field");

        assert!(matches!(&error, Error::Conflict(_)));
        assert!(error.to_string().contains("cannot be removed or renamed"));
    }

    #[test]
    fn populated_table_rejects_type_changes() {
        let error = validate_field_update(
            &table(1),
            vec![field("tier", "number", false, "Tier number")],
        )
        .expect_err("type change requires an empty table");

        assert!(matches!(&error, Error::Conflict(_)));
        assert!(error.to_string().contains("type cannot change"));
    }

    #[test]
    fn empty_table_allows_destructive_schema_changes() {
        let update = validate_field_update(
            &table(0),
            vec![field("plan", "number", false, "Replacement")],
        )
        .expect("empty table schema can be replaced");

        assert!(update.require_empty);
        assert_eq!(update.value_fields[0].name, "plan");
    }
}
