// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;

use super::{
    StatusPage, StatusPageComponent, StatusPageComponentStatusEvent, StatusPageDomainCheckUpdate,
    StatusPageDomainConfig, StatusPageDomainState, StatusPageEventList, StatusPageEventView,
    StatusPageHistoryPage, StatusPageHistoryQuery, StatusPageIncident, StatusPageIncidentKind,
    StatusPageIncidentUpdate,
};
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[async_trait]
pub trait StatusPagePageRepository: Send + Sync {
    async fn create_page(&self, page: StatusPage) -> Result<StatusPage>;
    async fn update_page(&self, page: StatusPage) -> Result<StatusPage>;
    async fn update_page_logo(
        &self,
        org_id: &Id,
        page_id: &Id,
        logo_url: Option<String>,
        updated_at: TimestampMicros,
    ) -> Result<StatusPage>;
    async fn delete_page(&self, org_id: &Id, page_id: &Id) -> Result<()>;
    async fn archive_page(
        &self,
        org_id: &Id,
        page_id: &Id,
        archived_at: TimestampMicros,
        purge_after: TimestampMicros,
    ) -> Result<StatusPage>;
    async fn restore_page(
        &self,
        org_id: &Id,
        page_id: &Id,
        updated_at: TimestampMicros,
    ) -> Result<StatusPage>;
    async fn purge_archived_pages(
        &self,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<StatusPage>>;
    async fn get_page(&self, org_id: &Id, page_id: &Id) -> Result<StatusPage>;
    async fn get_page_by_slug(&self, slug: &str) -> Result<Option<StatusPage>>;
    async fn get_public_page_by_slug(&self, slug: &str) -> Result<Option<StatusPage>>;
    /// Global lookup reserved for anonymous Host routing. `custom_domain` is
    /// case-insensitively unique across all organizations; callers must still
    /// enforce public visibility before projecting a response.
    async fn get_page_by_custom_domain(&self, domain: &str) -> Result<Option<StatusPage>>;
    async fn list_pages(&self, org_id: &Id) -> Result<Vec<StatusPage>>;
}

#[async_trait]
pub trait StatusPageComponentRepository: Send + Sync {
    async fn create_component(
        &self,
        component: StatusPageComponent,
        initial_event: StatusPageComponentStatusEvent,
    ) -> Result<StatusPageComponent>;
    async fn update_component(
        &self,
        component: StatusPageComponent,
        expected_status: super::ComponentStatus,
        status_event: Option<StatusPageComponentStatusEvent>,
    ) -> Result<StatusPageComponent>;
    async fn delete_component(
        &self,
        org_id: &Id,
        page_id: &Id,
        component_id: &Id,
        updated_at: TimestampMicros,
    ) -> Result<()>;
    async fn get_component(
        &self,
        org_id: &Id,
        page_id: &Id,
        component_id: &Id,
    ) -> Result<StatusPageComponent>;
    async fn list_components(&self, org_id: &Id, page_id: &Id) -> Result<Vec<StatusPageComponent>>;
    async fn list_component_status_events(
        &self,
        org_id: &Id,
        page_id: &Id,
        since: TimestampMicros,
        until: TimestampMicros,
    ) -> Result<Vec<StatusPageComponentStatusEvent>>;
}

#[async_trait]
pub trait StatusPageDomainRepository: Send + Sync {
    async fn create_domain_config(
        &self,
        config: StatusPageDomainConfig,
    ) -> Result<StatusPageDomainConfig>;
    async fn get_domain_config(
        &self,
        org_id: &Id,
        page_id: &Id,
    ) -> Result<Option<StatusPageDomainConfig>>;
    async fn list_domain_configs_due(
        &self,
        checked_before: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<StatusPageDomainConfig>>;
    async fn record_domain_check(
        &self,
        update: StatusPageDomainCheckUpdate,
    ) -> Result<StatusPageDomainConfig>;
    async fn retry_domain_tls(
        &self,
        org_id: &Id,
        page_id: &Id,
        updated_at: TimestampMicros,
    ) -> Result<StatusPageDomainConfig>;
    async fn mark_domain_health_alerted(
        &self,
        org_id: &Id,
        page_id: &Id,
        state: Option<StatusPageDomainState>,
        updated_at: TimestampMicros,
    ) -> Result<()>;
    async fn delete_domain_config(&self, org_id: &Id, page_id: &Id) -> Result<()>;
}

pub trait StatusPageConfigRepository:
    StatusPagePageRepository + StatusPageComponentRepository + StatusPageDomainRepository
{
}

impl<T> StatusPageConfigRepository for T where
    T: StatusPagePageRepository + StatusPageComponentRepository + StatusPageDomainRepository
{
}

#[async_trait]
pub trait StatusPageIncidentWriteRepository: Send + Sync {
    async fn create_status_incident(
        &self,
        incident: StatusPageIncident,
    ) -> Result<StatusPageIncident>;
    async fn append_status_update(
        &self,
        update: StatusPageIncidentUpdate,
        expected_status: super::PublicIncidentStatus,
        ended_at: Option<TimestampMicros>,
        updated_at: TimestampMicros,
    ) -> Result<StatusPageIncident>;
    async fn update_status_incident_draft(
        &self,
        incident: StatusPageIncident,
    ) -> Result<StatusPageIncident>;
    async fn update_status_incident_metadata(
        &self,
        incident: StatusPageIncident,
    ) -> Result<StatusPageIncident>;
    async fn publish_status_incident(
        &self,
        incident: StatusPageIncident,
        initial_update: StatusPageIncidentUpdate,
    ) -> Result<StatusPageIncident>;
    async fn update_scheduled_maintenance(
        &self,
        incident: StatusPageIncident,
        update: StatusPageIncidentUpdate,
    ) -> Result<StatusPageIncident>;
}

#[async_trait]
pub trait StatusPageIncidentQueryRepository: Send + Sync {
    async fn get_status_incident(
        &self,
        org_id: &Id,
        page_id: &Id,
        incident_id: &Id,
    ) -> Result<StatusPageIncident>;
    async fn list_status_incidents(
        &self,
        org_id: &Id,
        page_id: &Id,
        since: TimestampMicros,
    ) -> Result<Vec<StatusPageIncident>>;
    async fn list_status_events(
        &self,
        org_id: &Id,
        page_id: &Id,
        kind: StatusPageIncidentKind,
        view: StatusPageEventView,
        limit: u32,
    ) -> Result<StatusPageEventList>;
    async fn query_status_history(
        &self,
        org_id: &Id,
        page_id: &Id,
        query: &StatusPageHistoryQuery,
    ) -> Result<StatusPageHistoryPage>;
}

pub trait StatusPageIncidentRepository:
    StatusPageIncidentWriteRepository + StatusPageIncidentQueryRepository
{
}

impl<T> StatusPageIncidentRepository for T where
    T: StatusPageIncidentWriteRepository + StatusPageIncidentQueryRepository
{
}

pub trait StatusPageRepository:
    StatusPageConfigRepository
    + StatusPageIncidentRepository
    + super::StatusPageSubscriptionRepository
    + super::StatusPageAccessRepository
    + super::StatusPageAutomationRepository
{
}

impl<T> StatusPageRepository for T where
    T: StatusPageConfigRepository
        + StatusPageIncidentRepository
        + super::StatusPageSubscriptionRepository
        + super::StatusPageAccessRepository
        + super::StatusPageAutomationRepository
{
}
