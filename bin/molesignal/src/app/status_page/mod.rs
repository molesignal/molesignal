// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Status Page use cases. Each submodule owns one management responsibility;
//! public transport and PostgreSQL details remain outside this layer.

use std::{collections::HashSet, sync::Arc};

mod automation;
mod components;
mod custom_domain;
mod history;
mod incidents;
mod input;
mod notification_copy;
mod pages;
mod planned_maintenance;
mod postmortems;
mod private_access;
mod service_links;
mod subscription;
mod updates;
mod validation;

pub use automation::{
    AutomationApprovalInput, AutomationLifecycleInput, AutomationRuleInput,
    AutomationRuleOrderInput, AutomationSettingsInput, AutomationSimulation,
    StatusPageAlertIncidentSink, StatusPageSyntheticTransitionSink,
};
pub use custom_domain::{StatusPageDomainCapability, StatusPageDomainInstructions};
pub use input::{
    StatusPageAccessRuleInput, StatusPageComponentInput, StatusPageDomainInput,
    StatusPageIncidentInput, StatusPageIncidentUpdateInput, StatusPageInput,
    StatusPageServiceLinkInput,
};
pub use planned_maintenance::PlannedMaintenanceSweep;
pub use private_access::{
    StatusPageAccessMetadata, StatusPageAccessRequestOutcome, StatusPageAccessSessionOutcome,
    access_cookie_name, token_hash,
};
pub use subscription::{
    StatusPageImportProblem, StatusPageSlackAppSetting, StatusPageSubscriberImportReport,
    StatusPageSubscriptionInput, StatusPageSubscriptionOutcome,
};

use crate::{
    domain::{
        alerting::repositories::IncidentRepository,
        business_service::BusinessServiceRepository,
        i18n_language::I18nLanguageRepository,
        status_page::{
            StatusPageDomainHealthNotifier, StatusPageDomainVerifier, StatusPageNotificationSender,
            StatusPageRepository, StatusPageSlackApps,
        },
    },
    shared::{Error, Result, ids::Id},
};

const MICROS_PER_DAY: i64 = 24 * 60 * 60 * 1_000_000;
pub const DEFAULT_STATUS_PAGE_HISTORY_DAYS: i32 = 90;
pub const DEFAULT_STATUS_PAGE_DELIVERY_RETENTION_DAYS: i32 = 90;
pub const DEFAULT_STATUS_PAGE_PRIVATE_SESSION_DAYS: i32 = 7;
pub const MAX_STATUS_PAGE_HISTORY_DAYS: i32 = 365;
const MAX_STATUS_PAGES_PER_ORG: usize = 200;
const MAX_COMPONENTS_PER_PAGE: usize = 500;

pub struct StatusPageService {
    pub(super) repository: Arc<dyn StatusPageRepository>,
    pub(super) incidents: Arc<dyn IncidentRepository>,
    pub(super) notification_sender: Option<Arc<dyn StatusPageNotificationSender>>,
    pub(super) i18n_language: Option<Arc<dyn I18nLanguageRepository>>,
    pub(super) domain_verifier: Option<Arc<dyn StatusPageDomainVerifier>>,
    pub(super) managed_domain_tls_configured: bool,
    pub(super) domain_health_notifier: Option<Arc<dyn StatusPageDomainHealthNotifier>>,
    pub(super) external_url: String,
    pub(super) business_services: Option<Arc<dyn BusinessServiceRepository>>,
    pub(super) slack_apps: Option<Arc<dyn StatusPageSlackApps>>,
}

impl StatusPageService {
    pub fn new(
        repository: Arc<dyn StatusPageRepository>,
        incidents: Arc<dyn IncidentRepository>,
    ) -> Self {
        Self {
            repository,
            incidents,
            notification_sender: None,
            i18n_language: None,
            domain_verifier: None,
            managed_domain_tls_configured: false,
            domain_health_notifier: None,
            external_url: String::new(),
            business_services: None,
            slack_apps: None,
        }
    }

    /// `i18n_language` holds the copy of the emails that `sender` delivers.
    pub fn with_notifications(
        mut self,
        sender: Arc<dyn StatusPageNotificationSender>,
        i18n_language: Arc<dyn I18nLanguageRepository>,
        external_url: impl Into<String>,
    ) -> Self {
        self.notification_sender = Some(sender);
        self.i18n_language = Some(i18n_language);
        self.external_url = external_url.into();
        self
    }

    pub fn with_domain_verifier(mut self, verifier: Arc<dyn StatusPageDomainVerifier>) -> Self {
        self.domain_verifier = Some(verifier);
        self
    }

    pub fn with_managed_domain_tls(mut self, configured: bool) -> Self {
        self.managed_domain_tls_configured = configured;
        self
    }

    pub fn with_domain_health_notifier(
        mut self,
        notifier: Arc<dyn StatusPageDomainHealthNotifier>,
    ) -> Self {
        self.domain_health_notifier = Some(notifier);
        self
    }

    /// The business services components can stand for.
    pub fn with_business_services(
        mut self,
        repository: Arc<dyn BusinessServiceRepository>,
    ) -> Self {
        self.business_services = Some(repository);
        self
    }

    /// The Slack apps visitors add a page to their channels with: the one a
    /// page names in its settings.
    pub fn with_slack_apps(mut self, slack_apps: Arc<dyn StatusPageSlackApps>) -> Self {
        self.slack_apps = Some(slack_apps);
        self
    }

    fn business_services(&self) -> Result<&Arc<dyn BusinessServiceRepository>> {
        self.business_services
            .as_ref()
            .ok_or_else(|| Error::unavailable("business services are not configured"))
    }
}

fn deduplicate_ids(ids: Vec<Id>) -> Vec<Id> {
    let mut seen = HashSet::new();

    ids.into_iter()
        .filter(|id| seen.insert(id.as_str().to_string()))
        .collect()
}
