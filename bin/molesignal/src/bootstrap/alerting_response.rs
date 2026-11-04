// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 事件响应几项服务的装配：洞察里人的部分、相似事件与最近变更、值班日历与换班、修复动作、工单、重大
//! 事件与复盘。

use std::sync::Arc;

use super::core::Core;
use crate::{
    app::{
        alerting::{
            AlertInsights, CalendarFeeds, IncidentResponseService, PostmortemService,
            RelatedIncidents, RemediationService, ShiftSwaps, TicketService,
        },
        notify::NotifyEngine,
    },
    config::Settings,
    domain::{
        alerting::timeline::IncidentTimelineRepository,
        apm::ApmCatalogRepository,
        notify::repositories::{NotifyDeliveryRepository, UserNotifyPreferenceRepository},
    },
    infra::persistence::repositories::{
        calendar_feeds::PgCalendarFeedRepository,
        incident_response::PgIncidentResponseRepository,
        incidents::rca::PgIncidentRcaRepository,
        notify::{PgNotifyConnectorRepository, PgUserNotifyEndpointRepository},
        remediation::PgRemediationRepository,
        schedule_swaps::PgSwapRequestRepository,
        tickets::PgTicketRepository,
        workspace_preference_defaults::PgWorkspacePreferenceDefaultsRepository,
    },
};

/// 告警运行时已经建好、这几项服务要用的部件。
pub(super) struct ResponseInputs {
    pub(super) timeline: Arc<dyn IncidentTimelineRepository>,
    pub(super) deliveries: Arc<dyn NotifyDeliveryRepository>,
    pub(super) preferences: Arc<dyn UserNotifyPreferenceRepository>,
    pub(super) notify_engine: Arc<NotifyEngine>,
    pub(super) apm_catalog: Arc<dyn ApmCatalogRepository>,
}

pub(super) struct ResponseServices {
    pub(super) insights: Arc<AlertInsights>,
    pub(super) related: Arc<RelatedIncidents>,
    pub(super) calendar: Arc<CalendarFeeds>,
    pub(super) swaps: Arc<ShiftSwaps>,
    pub(super) remediation: Arc<RemediationService>,
    pub(super) tickets: Arc<TicketService>,
    pub(super) response: Arc<IncidentResponseService>,
    pub(super) postmortems: Arc<PostmortemService>,
}

impl ResponseServices {
    pub(super) fn build(settings: &Settings, core: &Core, inputs: ResponseInputs) -> Self {
        let ResponseInputs {
            timeline,
            deliveries,
            preferences,
            notify_engine,
            apm_catalog,
        } = inputs;
        let external_url = settings.http.external_url.clone();

        let insights = Arc::new(AlertInsights::new(
            core.incidents.clone(),
            timeline.clone(),
            core.schedules.clone(),
            deliveries,
            preferences,
        ));

        let related = Arc::new(RelatedIncidents::new(
            core.incidents.clone(),
            timeline.clone(),
            Arc::new(PgIncidentRcaRepository::new(core.pool.clone())),
            core.alert_rules.clone(),
            apm_catalog,
        ));

        let calendar = Arc::new(CalendarFeeds::new(
            Arc::new(PgCalendarFeedRepository::new(core.pool.clone())),
            core.schedules.clone(),
        ));

        let swaps = Arc::new(ShiftSwaps::new(
            Arc::new(PgSwapRequestRepository::new(core.pool.clone())),
            core.schedules.clone(),
            notify_engine.clone(),
        ));

        let tickets = Arc::new(TicketService::new(
            Arc::new(PgTicketRepository::new(
                core.pool.clone(),
                core.cipher_root_key.clone(),
            )),
            core.incidents.clone(),
            timeline.clone(),
            Arc::new(PgIncidentRcaRepository::new(core.pool.clone())),
            core.i18n_language.clone(),
            Arc::new(PgWorkspacePreferenceDefaultsRepository::new(
                core.pool.clone(),
            )),
            external_url.clone(),
        ));

        let remediation = Arc::new(RemediationService::new(
            Arc::new(PgRemediationRepository::new(
                core.pool.clone(),
                core.cipher_root_key.clone(),
            )),
            core.incidents.clone(),
            core.alert_rules.clone(),
            timeline.clone(),
            external_url.clone(),
        ));

        let response_repository = Arc::new(PgIncidentResponseRepository::new(
            core.pool.clone(),
            core.cipher_root_key.clone(),
        ));

        let response = Arc::new(IncidentResponseService::new(
            response_repository.clone(),
            core.incidents.clone(),
            timeline,
            Arc::new(PgNotifyConnectorRepository::new(
                core.pool.clone(),
                core.cipher_root_key.clone(),
            )),
            Arc::new(PgUserNotifyEndpointRepository::new(core.pool.clone())),
            notify_engine,
            tickets.clone(),
            external_url,
        ));

        let postmortems = Arc::new(PostmortemService::new(response_repository, tickets.clone()));

        Self {
            insights,
            related,
            calendar,
            swaps,
            remediation,
            tickets,
            response,
            postmortems,
        }
    }
}
