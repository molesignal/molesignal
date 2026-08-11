// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod canonical;
mod validator;

pub use canonical::{canonical_json, canonical_json_bytes, sha256_hex};
pub use validator::{
    ContractIssue, ContractSchemaError, ContractValidator, DASHBOARD_AUTHORING_V1_SCHEMA,
    DASHBOARD_MODEL_V2_SCHEMA, DASHBOARD_VISUALIZATIONS_V1, dashboard_authoring_validator,
    dashboard_model_validator,
};
