// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::{
    MAX_COMPONENTS_PER_PAGE, StatusPageComponentInput, StatusPageService,
    validation::validate_component_input,
};
use crate::{
    domain::status_page::{
        ComponentKind, ComponentLifecycle, ComponentStatus, ComponentVisibility,
        StatusPageComponent, StatusPageComponentStatusEvent, StatusPageLifecycle, group_members,
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

        let group_id = component_group(input.kind, input.group_id.clone(), &components)?;

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
        let (status, visibility) = shown_as(input.kind, input.status, input.visibility);

        self.repository
            .create_component(
                StatusPageComponent {
                    id: component_id.clone(),
                    org_id: org_id.clone(),
                    status_page_id: page_id.clone(),
                    name: input.name.trim().to_string(),
                    description: input.description.trim().to_string(),
                    status,
                    visibility,
                    lifecycle: ComponentLifecycle::Active,
                    position,
                    archived_at: None,
                    created_at: now,
                    updated_at: now,
                    business_service: None,
                    kind: input.kind,
                    group_id,
                },
                StatusPageComponentStatusEvent {
                    id: Id::new(),
                    org_id: org_id.clone(),
                    status_page_id: page_id.clone(),
                    component_id,
                    status,
                    started_at: now,
                    ended_at: None,
                    created_at: now,
                    notify_subscribers: input.notify_subscribers,
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

        // What a component is stays as it was made; a group keeps no status of its own.
        let kind = existing.kind;
        let (status, visibility) = shown_as(kind, input.status, input.visibility);

        if existing.lifecycle != input.lifecycle && existing.status != status {
            return Err(Error::invalid(
                "restore or archive a component separately from changing its status",
            ));
        }

        let components = self.repository.list_components(org_id, page_id).await?;
        let group_id = component_group(kind, input.group_id.clone(), &components)?;

        if kind == ComponentKind::Group
            && input.lifecycle == ComponentLifecycle::Archived
            && group_members(&components, &existing.id).next().is_some()
        {
            return Err(Error::conflict(
                "move the components out of the group, or archive them, before archiving the group",
            ));
        }

        let expected_status = existing.status;
        let now = TimestampMicros::now();

        let status_event = (existing.status != status).then(|| StatusPageComponentStatusEvent {
            id: Id::new(),
            org_id: org_id.clone(),
            status_page_id: page_id.clone(),
            component_id: component_id.clone(),
            status,
            started_at: now,
            ended_at: None,
            created_at: now,
            notify_subscribers: input.notify_subscribers,
        });

        self.repository
            .update_component(
                StatusPageComponent {
                    id: existing.id,
                    org_id: existing.org_id,
                    status_page_id: existing.status_page_id,
                    name: input.name.trim().to_string(),
                    description: input.description.trim().to_string(),
                    status,
                    visibility,
                    lifecycle: input.lifecycle,
                    position: input.position.unwrap_or(existing.position),
                    archived_at: match input.lifecycle {
                        ComponentLifecycle::Active => None,
                        ComponentLifecycle::Archived => existing.archived_at.or(Some(now)),
                    },
                    created_at: existing.created_at,
                    updated_at: now,
                    business_service: existing.business_service,
                    kind,
                    group_id,
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

/// The group a component goes in: a group of the same page that is not
/// archived. A group goes in none, as groups hold components and not groups.
fn component_group(
    kind: ComponentKind,
    group_id: Option<Id>,
    components: &[StatusPageComponent],
) -> Result<Option<Id>> {
    let Some(group_id) = group_id else {
        return Ok(None);
    };

    if kind == ComponentKind::Group {
        return Err(Error::invalid("a group cannot be put in another group"));
    }

    let group = components.iter().find(|component| component.id == group_id);

    match group {
        Some(group) if group.is_group() && group.lifecycle == ComponentLifecycle::Active => {
            Ok(Some(group_id))
        }
        _ => Err(Error::invalid(
            "a component can only be put in a group of its page that is not archived",
        )),
    }
}

/// The status and visibility a component is kept with. A group has no status
/// of its own and is shown while its components are.
fn shown_as(
    kind: ComponentKind,
    status: ComponentStatus,
    visibility: ComponentVisibility,
) -> (ComponentStatus, ComponentVisibility) {
    match kind {
        ComponentKind::Component => (status, visibility),
        ComponentKind::Group => (ComponentStatus::Operational, ComponentVisibility::Enabled),
    }
}
