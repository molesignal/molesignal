// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::{
    MAX_COMPONENTS_PER_PAGE, StatusPageComponentInput, StatusPageService,
    validation::validate_component_input,
};
use crate::{
    domain::status_page::{
        ComponentLifecycle, StatusPageComponent, StatusPageComponentStatusEvent,
        StatusPageLifecycle,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

impl StatusPageService {
    pub async fn create_component(
        &self,
        org_id: &Id,
        page_id: &Id,
        input: StatusPageComponentInput,
    ) -> Result<StatusPageComponent> {
        let page = self.repository.get_page(org_id, page_id).await?;
        if page.lifecycle == StatusPageLifecycle::Archived {
            return Err(Error::conflict(
                "components cannot be added to an archived status page",
            ));
        }
        validate_component_input(&input)?;
        let components = self.repository.list_components(org_id, page_id).await?;
        if components.len() >= MAX_COMPONENTS_PER_PAGE {
            return Err(Error::invalid(
                "a status page can have at most 500 components",
            ));
        }
        let position = input.position.unwrap_or_else(|| {
            components
                .iter()
                .map(|component| component.position)
                .max()
                .unwrap_or(-10)
                .saturating_add(10)
        });
        let now = TimestampMicros::now();
        let component_id = Id::new();
        self.repository
            .create_component(
                StatusPageComponent {
                    id: component_id.clone(),
                    org_id: org_id.clone(),
                    status_page_id: page_id.clone(),
                    name: input.name.trim().to_string(),
                    description: input.description.trim().to_string(),
                    status: input.status,
                    visibility: input.visibility,
                    lifecycle: ComponentLifecycle::Active,
                    position,
                    archived_at: None,
                    created_at: now,
                    updated_at: now,
                },
                StatusPageComponentStatusEvent {
                    id: Id::new(),
                    org_id: org_id.clone(),
                    status_page_id: page_id.clone(),
                    component_id,
                    status: input.status,
                    started_at: now,
                    ended_at: None,
                    created_at: now,
                },
            )
            .await
    }

    pub async fn update_component(
        &self,
        org_id: &Id,
        page_id: &Id,
        component_id: &Id,
        input: StatusPageComponentInput,
    ) -> Result<StatusPageComponent> {
        let page = self.repository.get_page(org_id, page_id).await?;
        if page.lifecycle == StatusPageLifecycle::Archived {
            return Err(Error::conflict(
                "components cannot be changed on an archived status page",
            ));
        }
        validate_component_input(&input)?;
        let existing = self
            .repository
            .get_component(org_id, page_id, component_id)
            .await?;
        if existing.lifecycle == ComponentLifecycle::Archived
            && input.lifecycle == ComponentLifecycle::Archived
        {
            return Err(Error::conflict("an archived component cannot be updated"));
        }
        if existing.lifecycle != input.lifecycle && existing.status != input.status {
            return Err(Error::invalid(
                "restore or archive a component separately from changing its status",
            ));
        }
        let expected_status = existing.status;
        let now = TimestampMicros::now();
        let status_event =
            (existing.status != input.status).then(|| StatusPageComponentStatusEvent {
                id: Id::new(),
                org_id: org_id.clone(),
                status_page_id: page_id.clone(),
                component_id: component_id.clone(),
                status: input.status,
                started_at: now,
                ended_at: None,
                created_at: now,
            });
        self.repository
            .update_component(
                StatusPageComponent {
                    id: existing.id,
                    org_id: existing.org_id,
                    status_page_id: existing.status_page_id,
                    name: input.name.trim().to_string(),
                    description: input.description.trim().to_string(),
                    status: input.status,
                    visibility: input.visibility,
                    lifecycle: input.lifecycle,
                    position: input.position.unwrap_or(existing.position),
                    archived_at: match input.lifecycle {
                        ComponentLifecycle::Active => None,
                        ComponentLifecycle::Archived => existing.archived_at.or(Some(now)),
                    },
                    created_at: existing.created_at,
                    updated_at: now,
                },
                expected_status,
                status_event,
            )
            .await
    }

    pub async fn delete_component(
        &self,
        org_id: &Id,
        page_id: &Id,
        component_id: &Id,
    ) -> Result<()> {
        let page = self.repository.get_page(org_id, page_id).await?;
        if page.lifecycle == StatusPageLifecycle::Archived {
            return Err(Error::conflict(
                "components cannot be deleted from an archived status page",
            ));
        }
        self.repository
            .delete_component(org_id, page_id, component_id, TimestampMicros::now())
            .await
    }
}
