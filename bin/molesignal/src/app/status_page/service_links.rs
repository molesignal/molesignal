// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Components that stand for business services. A component in follow mode
//! takes on the health of its service; one in automation mode is where the
//! page's business-service rules publish incidents.

use super::{StatusPageService, StatusPageServiceLinkInput};
use crate::{
    domain::{
        business_service::BusinessServiceStatus,
        status_page::{
            ActiveAutomationRule, AutomationSourceKind, AutomationSourceObservation,
            ComponentLifecycle, ComponentServiceLink, ComponentStatus, ServiceLinkMode,
            StatusPageComponent, StatusPageComponentStatusEvent, StatusPageLifecycle,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

impl StatusPageService {
    /// Links a component to a business service, or unlinks it with `None`. A
    /// component that follows the service takes on its health at once.
    pub async fn set_component_service_link(
        &self,
        org_id: &Id,
        page_id: &Id,
        component_id: &Id,
        input: Option<StatusPageServiceLinkInput>,
    ) -> Result<StatusPageComponent> {
        let page = self.repository.get_page(org_id, page_id).await?;

        if page.lifecycle == StatusPageLifecycle::Archived {
            return Err(Error::conflict(
                "components cannot be changed on an archived status page",
            ));
        }

        let component = self
            .repository
            .get_component(org_id, page_id, component_id)
            .await?;

        if component.lifecycle == ComponentLifecycle::Archived && input.is_some() {
            return Err(Error::conflict(
                "an archived component cannot stand for a business service",
            ));
        }

        if component.is_group() && input.is_some() {
            return Err(Error::invalid(
                "a group cannot stand for a business service; link a component in it",
            ));
        }

        let link = match input {
            Some(input) => {
                let service = self
                    .business_services()?
                    .get_business_service(org_id, &input.business_service_id)
                    .await?;

                let link = ComponentServiceLink {
                    business_service_id: service.id,
                    mode: input.mode,
                    notify_subscribers: input.notify_subscribers,
                };

                Some((link, service.health.status))
            }
            None => None,
        };

        let linked = self
            .repository
            .set_component_service_link(
                org_id,
                page_id,
                component_id,
                link.as_ref().map(|(link, _)| link),
                TimestampMicros::now(),
            )
            .await?;

        match link {
            Some((link, health)) if link.mode == ServiceLinkMode::Follow => {
                self.follow_health(linked, health.component_status(), link.notify_subscribers)
                    .await
            }
            _ => Ok(linked),
        }
    }

    /// Every component of the organization that stands for a business
    /// service, or for the one named, on pages that are not archived.
    pub async fn list_service_linked_components(
        &self,
        org_id: &Id,
        business_service_id: Option<&Id>,
    ) -> Result<Vec<StatusPageComponent>> {
        self.repository
            .list_service_linked_components(org_id, business_service_id)
            .await
    }

    /// Gives every component that follows the business service its health.
    /// Returns how many components changed status.
    pub async fn apply_business_service_health(
        &self,
        org_id: &Id,
        business_service_id: &Id,
        health: BusinessServiceStatus,
    ) -> Result<u32> {
        let components = self
            .repository
            .list_service_linked_components(org_id, Some(business_service_id))
            .await?;

        let mut changed = 0;

        for component in components {
            let Some(link) = component.business_service.clone() else {
                continue;
            };

            if link.mode != ServiceLinkMode::Follow || component.status == health.component_status()
            {
                continue;
            }

            let component_id = component.id.clone();

            match self
                .follow_health(
                    component,
                    health.component_status(),
                    link.notify_subscribers,
                )
                .await
            {
                Ok(_) => changed += 1,
                Err(error) => tracing::warn!(
                    component_id = %component_id,
                    business_service_id = %business_service_id,
                    error = %error,
                    "status-page component could not follow its business service"
                ),
            }
        }

        Ok(changed)
    }

    /// The components on the page that stand for the business service and
    /// publish through automation.
    pub(super) async fn automation_components(
        &self,
        org_id: &Id,
        page_id: &Id,
        business_service_id: &Id,
    ) -> Result<Vec<Id>> {
        Ok(self
            .repository
            .list_service_linked_components(org_id, Some(business_service_id))
            .await?
            .into_iter()
            .filter(|component| {
                &component.status_page_id == page_id
                    && component.lifecycle == ComponentLifecycle::Active
                    && component
                        .business_service
                        .as_ref()
                        .is_some_and(|link| link.mode == ServiceLinkMode::Automation)
            })
            .map(|component| component.id)
            .collect())
    }

    /// The components a matching automation rule publishes on: the ones it
    /// names, or for a business-service rule that names none, the components
    /// of its page that stand for the service and publish through automation.
    pub(in crate::app::status_page) async fn rule_components(
        &self,
        rule: &ActiveAutomationRule,
        observation: &AutomationSourceObservation,
    ) -> Result<Vec<Id>> {
        let named = &rule.revision.action.component_ids;

        if !named.is_empty() || observation.source_kind != AutomationSourceKind::BusinessService {
            return Ok(named.clone());
        }

        self.automation_components(
            &observation.organization_id,
            &rule.rule.status_page_id,
            &observation.source_id,
        )
        .await
    }

    async fn follow_health(
        &self,
        component: StatusPageComponent,
        status: ComponentStatus,
        notify_subscribers: bool,
    ) -> Result<StatusPageComponent> {
        if component.status == status || component.lifecycle == ComponentLifecycle::Archived {
            return Ok(component);
        }

        let now = TimestampMicros::now();
        let expected_status = component.status;

        let event = StatusPageComponentStatusEvent {
            id: Id::new(),
            org_id: component.org_id.clone(),
            status_page_id: component.status_page_id.clone(),
            component_id: component.id.clone(),
            status,
            started_at: now,
            ended_at: None,
            created_at: now,
            notify_subscribers,
        };

        self.repository
            .update_component(
                StatusPageComponent {
                    status,
                    updated_at: now,
                    ..component
                },
                expected_status,
                Some(event),
            )
            .await
    }
}
