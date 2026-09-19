// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashSet;

use super::{
    MAX_STATUS_PAGES_PER_ORG, MICROS_PER_DAY, StatusPageDomainInput, StatusPageInput,
    StatusPageService,
    validation::{normalize_domain_lookup, normalize_logo_url, normalize_page_input},
};
use crate::{
    domain::status_page::{
        ComponentLifecycle, ComponentVisibility, StatusPage, StatusPageLifecycle,
        StatusPageSnapshot,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const ARCHIVE_RETENTION_DAYS: i64 = 30;

impl StatusPageService {
    pub async fn list_pages(
        &self,
        org_id: &Id,
        lifecycle: Option<StatusPageLifecycle>,
    ) -> Result<Vec<StatusPage>> {
        let pages = self.repository.list_pages(org_id).await?;
        Ok(pages
            .into_iter()
            .filter(|page| lifecycle.is_none_or(|value| page.lifecycle == value))
            .collect())
    }

    pub async fn get_page(&self, org_id: &Id, page_id: &Id) -> Result<StatusPage> {
        self.repository.get_page(org_id, page_id).await
    }

    pub async fn create_page(&self, org_id: &Id, input: StatusPageInput) -> Result<StatusPage> {
        let input = normalize_page_input(input)?;
        if self.repository.list_pages(org_id).await?.len() >= MAX_STATUS_PAGES_PER_ORG {
            return Err(Error::invalid(
                "an organization can have at most 200 status pages",
            ));
        }
        let custom_domain = input.custom_domain.clone();
        let now = TimestampMicros::now();
        let page = self
            .repository
            .create_page(StatusPage {
                id: Id::new(),
                org_id: org_id.clone(),
                name: input.name,
                slug: input.slug,
                logo_url: input.logo_url,
                brand_color: input.brand_color,
                custom_domain: None,
                timezone: input.timezone,
                language: input.language,
                languages: input.languages,
                history_days: input.history_days,
                delivery_retention_days: input.delivery_retention_days,
                private_session_days: input.private_session_days,
                visibility: input.visibility,
                lifecycle: StatusPageLifecycle::Active,
                archived_at: None,
                purge_after: None,
                created_at: now,
                updated_at: now,
            })
            .await?;
        if let Some(hostname) = custom_domain {
            self.configure_custom_domain(org_id, &page.id, StatusPageDomainInput { hostname })
                .await?;
            return self.repository.get_page(org_id, &page.id).await;
        }
        Ok(page)
    }

    pub async fn update_page(
        &self,
        org_id: &Id,
        page_id: &Id,
        input: StatusPageInput,
    ) -> Result<StatusPage> {
        let input = normalize_page_input(input)?;
        let existing = self.repository.get_page(org_id, page_id).await?;
        if existing.lifecycle == StatusPageLifecycle::Archived {
            return Err(Error::conflict("an archived status page cannot be updated"));
        }
        let desired_domain = input.custom_domain.clone();
        let updated = self
            .repository
            .update_page(StatusPage {
                id: existing.id,
                org_id: existing.org_id,
                name: input.name,
                slug: input.slug,
                logo_url: input.logo_url,
                brand_color: input.brand_color,
                custom_domain: existing.custom_domain.clone(),
                timezone: input.timezone,
                language: input.language,
                languages: input.languages,
                history_days: input.history_days,
                delivery_retention_days: input.delivery_retention_days,
                private_session_days: input.private_session_days,
                visibility: input.visibility,
                lifecycle: existing.lifecycle,
                archived_at: existing.archived_at,
                purge_after: existing.purge_after,
                created_at: existing.created_at,
                updated_at: TimestampMicros::now(),
            })
            .await?;
        if desired_domain != existing.custom_domain {
            if let Some(hostname) = desired_domain {
                self.configure_custom_domain(org_id, page_id, StatusPageDomainInput { hostname })
                    .await?;
            } else {
                self.remove_custom_domain(org_id, page_id).await?;
            }
            return self.repository.get_page(org_id, page_id).await;
        }
        Ok(updated)
    }

    pub async fn update_logo_url(
        &self,
        org_id: &Id,
        page_id: &Id,
        logo_url: Option<String>,
    ) -> Result<StatusPage> {
        let page = self.repository.get_page(org_id, page_id).await?;
        if page.lifecycle == StatusPageLifecycle::Archived {
            return Err(Error::conflict("an archived status page cannot be updated"));
        }
        self.repository
            .update_page_logo(
                org_id,
                page_id,
                normalize_logo_url(logo_url)?,
                TimestampMicros::now(),
            )
            .await
    }

    pub async fn archive_page(&self, org_id: &Id, page_id: &Id) -> Result<StatusPage> {
        let now = TimestampMicros::now();
        self.repository
            .archive_page(
                org_id,
                page_id,
                now,
                TimestampMicros(
                    now.0
                        .saturating_add(ARCHIVE_RETENTION_DAYS.saturating_mul(MICROS_PER_DAY)),
                ),
            )
            .await
    }

    pub async fn restore_page(&self, org_id: &Id, page_id: &Id) -> Result<StatusPage> {
        self.repository
            .restore_page(org_id, page_id, TimestampMicros::now())
            .await
    }

    pub async fn delete_page(&self, org_id: &Id, page_id: &Id) -> Result<()> {
        let page = self.repository.get_page(org_id, page_id).await?;
        if page.lifecycle != StatusPageLifecycle::Archived {
            return Err(Error::conflict(
                "a status page must be archived before it can be permanently deleted",
            ));
        }
        self.repository.delete_page(org_id, page_id).await
    }

    pub async fn purge_archived_pages(&self, limit: u32) -> Result<Vec<StatusPage>> {
        self.repository
            .purge_archived_pages(TimestampMicros::now(), limit.min(100))
            .await
    }

    pub async fn get_snapshot(&self, org_id: &Id, page_id: &Id) -> Result<StatusPageSnapshot> {
        let page = self.repository.get_page(org_id, page_id).await?;
        self.build_snapshot(page, false).await
    }

    pub async fn get_public_page(&self, slug: &str) -> Result<StatusPage> {
        let normalized = slug.trim().to_ascii_lowercase();
        self.repository
            .get_public_page_by_slug(&normalized)
            .await?
            .ok_or_else(|| Error::not_found("status page not found"))
    }

    pub(super) async fn get_customer_page(&self, slug: &str) -> Result<StatusPage> {
        let normalized = slug.trim().to_ascii_lowercase();
        let page = self
            .repository
            .get_page_by_slug(&normalized)
            .await?
            .ok_or_else(|| Error::not_found("status page not found"))?;
        if page.lifecycle != StatusPageLifecycle::Active {
            return Err(Error::not_found("status page not found"));
        }
        Ok(page)
    }

    pub async fn get_customer_branding_page(&self, slug: &str) -> Result<StatusPage> {
        self.get_customer_page(slug).await
    }

    pub async fn get_public_snapshot(&self, slug: &str) -> Result<StatusPageSnapshot> {
        let page = self.get_public_page(slug).await?;
        self.build_snapshot(page, true).await
    }

    pub async fn resolve_customer_page_by_domain(
        &self,
        domain: &str,
    ) -> Result<Option<StatusPage>> {
        let Some(domain) = normalize_domain_lookup(domain) else {
            return Ok(None);
        };
        self.repository.get_page_by_custom_domain(&domain).await
    }

    pub(super) async fn build_snapshot(
        &self,
        page: StatusPage,
        public_view: bool,
    ) -> Result<StatusPageSnapshot> {
        let now = TimestampMicros::now();
        let query_days = page.history_days.max(1);
        let history_window_micros = i64::from(query_days).saturating_mul(MICROS_PER_DAY);
        let since = TimestampMicros(now.0.saturating_sub(history_window_micros));
        let components = self
            .repository
            .list_components(&page.org_id, &page.id)
            .await?;
        let incidents = self
            .repository
            .list_status_incidents(&page.org_id, &page.id, since)
            .await?;
        let component_status_events = self
            .repository
            .list_component_status_events(&page.org_id, &page.id, since, now)
            .await?;
        let mut snapshot =
            StatusPageSnapshot::build(page, components, component_status_events, incidents, now);
        if public_view {
            let visible_ids: HashSet<String> = snapshot
                .components
                .iter()
                .filter(|component| {
                    component.visibility == ComponentVisibility::Enabled
                        && component.lifecycle == ComponentLifecycle::Active
                })
                .map(|component| component.id.0.clone())
                .collect();
            snapshot
                .components
                .retain(|component| visible_ids.contains(component.id.as_str()));
            snapshot
                .component_status_events
                .retain(|event| visible_ids.contains(event.component_id.as_str()));
            for incident in snapshot
                .active_incidents
                .iter_mut()
                .chain(snapshot.scheduled_maintenance.iter_mut())
                .chain(snapshot.history.iter_mut())
            {
                incident
                    .component_ids
                    .retain(|component_id| visible_ids.contains(component_id.as_str()));
            }
            for incident in &snapshot.active_incidents {
                let incident_status = incident.impact.component_status();
                for component in &mut snapshot.components {
                    if incident.component_ids.contains(&component.id) {
                        component.status = crate::domain::status_page::ComponentStatus::worst(
                            component.status,
                            incident_status,
                        );
                    }
                }
            }
            snapshot.overall_status = snapshot.components.iter().fold(
                crate::domain::status_page::ComponentStatus::Operational,
                |current, component| {
                    crate::domain::status_page::ComponentStatus::worst(current, component.status)
                },
            );
        }
        Ok(snapshot)
    }
}
