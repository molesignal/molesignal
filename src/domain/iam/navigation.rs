// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Database-backed product route and navigation policy contracts.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IamRouteScope {
    Any,
    Organization,
    System,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IamRoutePermissionMode {
    All,
    Any,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IamRouteDefinition {
    pub id: String,
    pub path_pattern: String,
    pub scope: IamRouteScope,
    pub permission_mode: IamRoutePermissionMode,
    pub permissions: Vec<String>,
    pub required_features: Vec<String>,
    pub navigation_group: Option<String>,
    pub navigation_position: Option<i32>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IamRouteCatalog {
    pub version: u64,
    pub routes: Vec<IamRouteDefinition>,
}

/// Route decision returned with the capability snapshot.
///
/// The browser receives the decision, not the permission expression. That
/// keeps route/menu rendering generic while the database remains the single
/// source of truth for the permission relationship.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IamRouteAccess {
    pub id: String,
    pub path_pattern: String,
    pub allowed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub navigation_group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub navigation_position: Option<i32>,
}
