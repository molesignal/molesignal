// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Organization-level resource limits shared by persistence and runtime enforcement.

/// Configured limits for one organization. A zero value means unlimited.
#[derive(Debug, Clone, Copy, Default)]
pub struct OrgQuota {
    pub max_intake_qps: u32,
    pub max_query_qps: u32,
    pub max_storage_bytes: u64,
    pub max_streams: u32,
}
