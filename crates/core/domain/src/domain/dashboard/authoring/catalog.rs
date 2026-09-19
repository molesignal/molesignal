// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashSet, sync::LazyLock};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::shared::contracts::DASHBOARD_VISUALIZATIONS_V1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VisualizationManifest {
    #[serde(rename = "$schema")]
    pub schema: String,
    #[serde(rename = "$id")]
    pub id: String,
    pub manifest_version: u32,
    pub dashboard_model_version: u32,
    pub authoring_versions: Vec<u32>,
    pub compiler_version: String,
    pub grid: ManifestGrid,
    pub limits: ManifestLimits,
    pub query_kinds: Vec<String>,
    pub units: Vec<String>,
    pub reducers: Vec<String>,
    pub visualizations: Vec<VisualizationCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestGrid {
    pub columns: u32,
    pub row_height: u32,
    pub gap: u32,
    pub sizes: ManifestSizes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestSizes {
    pub small: GridDimensions,
    pub medium: GridDimensions,
    pub wide: GridDimensions,
    pub full: GridDimensions,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridDimensions {
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestLimits {
    pub max_panels: usize,
    pub max_elements: usize,
    pub max_queries_per_panel: usize,
    pub max_variables: usize,
    pub max_query_length: usize,
    pub max_lookback_seconds: u64,
    pub preflight_timeout_ms: u64,
    pub preflight_max_rows: usize,
    pub preflight_max_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VisualizationCapability {
    #[serde(rename = "type")]
    pub visualization_type: String,
    pub option_schema_version: u32,
    pub default_size: String,
    pub default_options: Map<String, Value>,
    pub compatible_query_kinds: Vec<String>,
    pub compatible_data_shapes: Vec<String>,
    pub allowed_units: Vec<String>,
    pub allowed_reducers: Vec<String>,
}

impl VisualizationManifest {
    pub fn from_value(value: Value) -> Result<Self, String> {
        let manifest: Self = serde_json::from_value(value)
            .map_err(|error| format!("Dashboard visualization manifest: {error}"))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn from_json(source: &str) -> Result<Self, String> {
        let value = serde_json::from_str(source)
            .map_err(|error| format!("Dashboard visualization manifest JSON: {error}"))?;
        Self::from_value(value)
    }

    #[must_use]
    pub fn visualization(&self, name: &str) -> Option<&VisualizationCapability> {
        self.visualizations
            .iter()
            .find(|candidate| candidate.visualization_type == name)
    }

    fn validate(&self) -> Result<(), String> {
        if self.manifest_version != 1
            || self.dashboard_model_version != 2
            || !self.authoring_versions.contains(&1)
        {
            return Err("unsupported Dashboard visualization manifest version".into());
        }
        if self.grid.columns != 24 || self.grid.row_height == 0 {
            return Err("Dashboard authoring grid must use 24 non-zero columns".into());
        }
        let query_kinds = self
            .query_kinds
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let units = self
            .units
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let reducers = self
            .reducers
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let mut visualization_types = HashSet::new();
        for capability in &self.visualizations {
            if !visualization_types.insert(capability.visualization_type.as_str()) {
                return Err(format!(
                    "duplicate visualization capability: {}",
                    capability.visualization_type
                ));
            }
            if capability.option_schema_version == 0
                || !matches!(
                    capability.default_size.as_str(),
                    "small" | "medium" | "wide" | "full"
                )
            {
                return Err(format!(
                    "invalid defaults for visualization {}",
                    capability.visualization_type
                ));
            }
            if !capability
                .compatible_query_kinds
                .iter()
                .all(|kind| query_kinds.contains(kind.as_str()))
                || !capability
                    .allowed_units
                    .iter()
                    .all(|unit| units.contains(unit.as_str()))
                || !capability
                    .allowed_reducers
                    .iter()
                    .all(|reducer| reducers.contains(reducer.as_str()))
            {
                return Err(format!(
                    "visualization {} references an unknown catalog value",
                    capability.visualization_type
                ));
            }
        }
        for dimensions in [
            self.grid.sizes.small,
            self.grid.sizes.medium,
            self.grid.sizes.wide,
            self.grid.sizes.full,
        ] {
            if dimensions.w == 0 || dimensions.w > self.grid.columns || dimensions.h == 0 {
                return Err("invalid Dashboard authoring grid dimensions".into());
            }
        }
        Ok(())
    }
}

static VISUALIZATION_MANIFEST: LazyLock<VisualizationManifest> = LazyLock::new(|| {
    VisualizationManifest::from_json(DASHBOARD_VISUALIZATIONS_V1)
        .expect("embedded Dashboard visualization manifest must be valid")
});

#[must_use]
pub fn visualization_manifest() -> &'static VisualizationManifest {
    &VISUALIZATION_MANIFEST
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_manifest_is_valid_and_queryable() {
        let manifest = visualization_manifest();
        assert_eq!(manifest.dashboard_model_version, 2);
        assert_eq!(manifest.grid.columns, 24);
        assert_eq!(
            manifest.visualization("time_series").unwrap().default_size,
            "medium"
        );
    }
}
