// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Status page components that stand for business services.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::StatusPageComponent;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

/// How the health of a business service reaches the component that stands
/// for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceLinkMode {
    /// The component's status follows the health of the service.
    Follow,
    /// The health is a source of the page's automation rules, which publish
    /// incidents on the component.
    Automation,
}

impl ServiceLinkMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Follow => "follow",
            Self::Automation => "automation",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "follow" => Some(Self::Follow),
            "automation" => Some(Self::Automation),
            _ => None,
        }
    }
}

/// The business service a component stands for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentServiceLink {
    pub business_service_id: Id,
    pub mode: ServiceLinkMode,
    /// Whether a status the component follows tells subscribers.
    pub notify_subscribers: bool,
}

#[async_trait]
pub trait StatusPageServiceLinkRepository: Send + Sync {
    /// Links a component to a business service, or unlinks it with `None`.
    async fn set_component_service_link(
        &self,
        org_id: &Id,
        page_id: &Id,
        component_id: &Id,
        link: Option<&ComponentServiceLink>,
        updated_at: TimestampMicros,
    ) -> Result<StatusPageComponent>;

    /// The components of the organization that stand for a business service,
    /// or for the one named, on pages that are not archived.
    async fn list_service_linked_components(
        &self,
        org_id: &Id,
        business_service_id: Option<&Id>,
    ) -> Result<Vec<StatusPageComponent>>;
}
