// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod catalog;
mod contract;
mod draft;
mod preflight;
mod repositories;

pub use catalog::{
    GridDimensions, ManifestGrid, ManifestLimits, VisualizationCapability, VisualizationManifest,
    visualization_manifest,
};
pub use contract::{
    AuthoringElement, AuthoringLegend, AuthoringQuery, AuthoringRefresh, AuthoringSection,
    AuthoringSize, AuthoringThreshold, AuthoringTimeRange, AuthoringVariable,
    DashboardAuthoringSpec, LegendMode, LegendPlacement, PanelAuthoringSpec, SectionElement,
    TextAuthoringSpec, TextMode, ThresholdMode, VisualizationIntent,
};
pub use draft::{
    DashboardAuthoringCapabilities, DashboardDraft, DashboardDraftStatus, DraftConsumption,
    PreflightWarningRecord, PreparedDashboardDraft,
};
pub use preflight::{
    DashboardQueryPreflight, PanelPreflight, PreflightReport, PreflightStatus, PreflightWarning,
};
pub use repositories::{ConsumeDashboardDraft, DashboardDraftRepository};
