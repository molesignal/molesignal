// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    L0,
    L1,
    L2,
    L3,
    L4,
}

impl RiskLevel {
    pub const fn required_approvals(self) -> i32 {
        match self {
            Self::L0 | Self::L1 => 0,
            Self::L2 => 1,
            Self::L3 | Self::L4 => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolAccess {
    ReadOnly,
    Preflight,
    ManagedMutation,
    CreatesApprovalRequest,
    ExecutesApprovedOperation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    All,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolAnnotations {
    pub read_only: bool,
    pub destructive: bool,
    pub idempotent: bool,
    pub open_world: bool,
    pub supports_dry_run: bool,
    pub streaming: bool,
}

impl ToolAnnotations {
    pub const READ_ONLY: Self = Self {
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: false,
        supports_dry_run: false,
        streaming: false,
    };

    pub const PROPOSAL: Self = Self {
        read_only: false,
        destructive: false,
        idempotent: false,
        open_world: false,
        supports_dry_run: true,
        streaming: false,
    };

    pub const PREFLIGHT: Self = Self {
        read_only: false,
        destructive: false,
        idempotent: false,
        open_world: false,
        supports_dry_run: true,
        streaming: false,
    };

    pub const APPROVED_EXECUTION: Self = Self {
        read_only: false,
        destructive: false,
        idempotent: true,
        open_world: false,
        supports_dry_run: false,
        streaming: false,
    };

    pub fn capabilities(self) -> Value {
        json!({
            "read_only": self.read_only,
            "destructive": self.destructive,
            "idempotent": self.idempotent,
            "open_world": self.open_world,
            "supports_dry_run": self.supports_dry_run,
            "streaming": self.streaming,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolExposure {
    pub pinned: bool,
    pub mole_agent: bool,
    pub inbound_mcp: bool,
    pub automation: bool,
    pub admin_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSurface {
    MoleAgent,
    InboundMcp,
    Automation,
}

impl ToolExposure {
    pub const DEFERRED: Self = Self {
        pinned: false,
        mole_agent: true,
        inbound_mcp: true,
        automation: false,
        admin_only: false,
    };

    pub const PINNED: Self = Self {
        pinned: true,
        ..Self::DEFERRED
    };

    pub const META: Self = Self {
        pinned: true,
        mole_agent: true,
        inbound_mcp: false,
        automation: false,
        admin_only: false,
    };

    pub const MOLE_AGENT_ONLY: Self = Self {
        pinned: false,
        mole_agent: true,
        inbound_mcp: false,
        automation: false,
        admin_only: false,
    };

    pub const INBOUND_MCP_ONLY: Self = Self {
        pinned: false,
        mole_agent: false,
        inbound_mcp: true,
        automation: false,
        admin_only: false,
    };

    pub const fn available_on(self, surface: ToolSurface) -> bool {
        match surface {
            ToolSurface::MoleAgent => self.mole_agent,
            ToolSurface::InboundMcp => self.inbound_mcp,
            ToolSurface::Automation => self.automation,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    /// Canonical en-US fallback generated from the stable tool name.
    pub display_name: String,
    /// Canonical MCP and LLM description. UI translations live outside the runtime catalog.
    pub description: String,
    pub domain: String,
    pub category: String,
    pub input_schema: Value,
    pub output_schema: Value,
    pub risk: RiskLevel,
    pub access: ToolAccess,
    pub annotations: ToolAnnotations,
    pub exposure: ToolExposure,
    pub required_permissions: Vec<String>,
    pub permission_mode: PermissionMode,
    pub tags: Vec<String>,
}

impl ToolSpec {
    #[allow(clippy::too_many_arguments)]
    pub fn read(
        name: &str,
        description: &str,
        domain: &str,
        category: &str,
        input_schema: Value,
        output_schema: Value,
        permissions: &[&str],
        tags: &[&str],
    ) -> Self {
        Self {
            name: name.into(),
            display_name: canonical_display_name(name),
            description: description.into(),
            domain: domain.into(),
            category: category.into(),
            input_schema,
            output_schema,
            risk: RiskLevel::L0,
            access: ToolAccess::ReadOnly,
            annotations: ToolAnnotations::READ_ONLY,
            exposure: ToolExposure::DEFERRED,
            required_permissions: permissions.iter().map(|value| (*value).into()).collect(),
            permission_mode: PermissionMode::All,
            tags: tags.iter().map(|value| (*value).into()).collect(),
        }
    }

    pub fn pinned(mut self) -> Self {
        self.exposure = ToolExposure::PINNED;
        self
    }

    pub fn meta(mut self) -> Self {
        self.exposure = ToolExposure::META;
        self
    }

    pub fn mole_agent_only(mut self) -> Self {
        let pinned = self.exposure.pinned;
        self.exposure = ToolExposure {
            pinned,
            ..ToolExposure::MOLE_AGENT_ONLY
        };
        self
    }

    pub fn inbound_mcp_only(mut self) -> Self {
        self.exposure = ToolExposure::INBOUND_MCP_ONLY;
        self
    }

    pub fn any_permission(mut self) -> Self {
        self.permission_mode = PermissionMode::Any;
        self
    }

    pub fn proposal(mut self, risk: RiskLevel) -> Self {
        self.risk = risk;
        self.access = ToolAccess::CreatesApprovalRequest;
        self.annotations = ToolAnnotations::PROPOSAL;
        self
    }

    pub fn preflight(mut self) -> Self {
        self.access = ToolAccess::Preflight;
        self.annotations = ToolAnnotations::PREFLIGHT;
        self
    }

    pub fn managed_mutation(
        mut self,
        risk: RiskLevel,
        destructive: bool,
        idempotent: bool,
    ) -> Self {
        self.risk = risk;
        self.access = ToolAccess::ManagedMutation;
        self.annotations = ToolAnnotations {
            read_only: false,
            destructive,
            idempotent,
            open_world: false,
            supports_dry_run: true,
            streaming: false,
        };
        self
    }

    /// Execute an already-approved operation without creating a nested approval request.
    pub fn approved_execution(mut self) -> Self {
        self.risk = RiskLevel::L1;
        self.access = ToolAccess::ExecutesApprovedOperation;
        self.annotations = ToolAnnotations::APPROVED_EXECUTION;
        self
    }
}

fn canonical_display_name(name: &str) -> String {
    match name {
        "tool_search" => return "Search tools".into(),
        "tools_call" => return "Call tool".into(),
        _ => {}
    }

    let mut words = name
        .split('_')
        .map(|word| match word {
            "ai" => "AI".into(),
            "apm" => "APM".into(),
            "dag" => "DAG".into(),
            "iam" => "IAM".into(),
            "kv" => "KV".into(),
            "llm" => "LLM".into(),
            "mcp" => "MCP".into(),
            "promql" => "PromQL".into(),
            "rca" => "RCA".into(),
            "rum" => "RUM".into(),
            "sql" => "SQL".into(),
            _ => word.into(),
        })
        .collect::<Vec<String>>();

    if let Some(first) = words.first_mut()
        && first.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
    {
        let initial = first[..1].to_ascii_uppercase();
        first.replace_range(..1, &initial);
    }

    words.join(" ").replace("on call", "on-call")
}

#[cfg(test)]
mod tests {
    use super::canonical_display_name;

    #[test]
    fn canonical_display_names_are_readable_and_preserve_acronyms() {
        assert_eq!(
            canonical_display_name("list_status_pages"),
            "List status pages"
        );
        assert_eq!(
            canonical_display_name("get_incident_rca"),
            "Get incident RCA"
        );
        assert_eq!(canonical_display_name("apm_overview"), "APM overview");
        assert_eq!(
            canonical_display_name("list_on_call_schedules"),
            "List on-call schedules"
        );
        assert_eq!(canonical_display_name("tool_search"), "Search tools");
        assert_eq!(canonical_display_name("tools_call"), "Call tool");
    }
}
