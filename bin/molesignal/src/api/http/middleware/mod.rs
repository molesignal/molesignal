// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! axum 中间件：JWT 鉴权 + 权限提取。

pub mod auth;

pub mod organization_status;
pub mod permission;
pub mod rate_limit;
pub mod read_grants;
pub mod trace_context;

pub use auth::auth_layer;
pub use organization_status::organization_status_layer;
pub use permission::{
    Permission, ProtectedResource, authorize_resource, authorize_resource_all_with,
    authorize_resource_any, authorize_resource_with,
};
pub use rate_limit::{RouterRateLimits, rate_limit_layer};
pub use read_grants::read_grants_layer;
pub use trace_context::trace_context_layer;
