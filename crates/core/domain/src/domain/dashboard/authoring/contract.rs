// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DashboardAuthoringSpec {
    pub authoring_version: u32,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_range: Option<AuthoringTimeRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh: Option<AuthoringRefresh>,
    #[serde(default)]
    pub variables: Vec<AuthoringVariable>,
    pub elements: Vec<AuthoringElement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthoringTimeRange {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum AuthoringRefresh {
    Off,
    Interval { interval: String },
    Live,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AuthoringElement {
    Panel(PanelAuthoringSpec),
    Text(TextAuthoringSpec),
    Section(AuthoringSection),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PanelAuthoringSpec {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<AuthoringSize>,
    pub visualization: VisualizationIntent,
    pub queries: Vec<AuthoringQuery>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transparent: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextAuthoringSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<TextMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<AuthoringSize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transparent: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthoringSection {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub collapsed: bool,
    pub elements: Vec<SectionElement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SectionElement {
    Panel(PanelAuthoringSpec),
    Text(TextAuthoringSpec),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextMode {
    Markdown,
    Plain,
}

impl TextMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Plain => "plain",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthoringSize {
    Small,
    Medium,
    Wide,
    Full,
}

impl AuthoringSize {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Wide => "wide",
            Self::Full => "full",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VisualizationIntent {
    #[serde(rename = "type")]
    pub visualization_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reducer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold_mode: Option<ThresholdMode>,
    #[serde(default)]
    pub thresholds: Vec<AuthoringThreshold>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legend: Option<AuthoringLegend>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThresholdMode {
    Absolute,
    Percentage,
}

impl ThresholdMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Absolute => "absolute",
            Self::Percentage => "percentage",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthoringThreshold {
    pub value: Option<f64>,
    pub color: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthoringLegend {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<LegendMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<LegendPlacement>,
    #[serde(default)]
    pub stats: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegendMode {
    Hidden,
    List,
    Table,
}

impl LegendMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hidden => "hidden",
            Self::List => "list",
            Self::Table => "table",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegendPlacement {
    Bottom,
    Right,
}

impl LegendPlacement {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bottom => "bottom",
            Self::Right => "right",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AuthoringQuery {
    Promql {
        expression: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stream: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        legend: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step: Option<String>,
    },
    Sql {
        stream: String,
        statement: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        time_column: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        legend: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
    },
    Trace {
        stream: String,
        query: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        legend: Option<String>,
    },
    Profile {
        stream: String,
        query: String,
        profile_type: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        aggregate: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        legend: Option<String>,
    },
}

impl AuthoringQuery {
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Promql { .. } => "promql",
            Self::Sql { .. } => "sql",
            Self::Trace { .. } => "trace",
            Self::Profile { .. } => "profile",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AuthoringVariable {
    Custom {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        values: Vec<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default_value: Option<Value>,
        #[serde(default)]
        multi: bool,
        #[serde(default)]
        include_all: bool,
    },
    Query {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        query: AuthoringQuery,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default_value: Option<Value>,
        #[serde(default)]
        multi: bool,
        #[serde(default)]
        include_all: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refresh: Option<String>,
    },
    Constant {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        value: Value,
        #[serde(default)]
        hidden: bool,
    },
    Text {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        value: Value,
        #[serde(default)]
        hidden: bool,
    },
    Interval {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        values: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default_value: Option<String>,
    },
    DataSource {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        data_source_type: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default_value: Option<String>,
    },
}
