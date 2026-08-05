// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警、通知与 dashboard 运行时装配。

use std::sync::Arc;

use super::{core::Core, query::QueryRuntime};
use crate::{
    app::{
        alerting::{AlertingService, EscalationDispatcher, RuleEvaluator},
        dashboard::{DashboardService, contract_registry::DashboardContractResolver},
        notify::{
            AlertOwnerResolver, ConnectorRegistry, CurrentOncallResolver, EventUsersResolver,
            FixedUsersResolver, NextOncallResolver, NotifyEngine, NotifyEngineDependencies,
            NotifyService, OncallEventProducer, RecipientResolverRegistry, ScheduleMembersResolver,
            TeamLeadResolver, TeamMembersResolver,
        },
    },
    config::Settings,
    domain::{
        alerting::{
            incident_group::IncidentGroupRepository, mute::MuteRuleRepository,
            semantic_group::SemanticGroupRepository,
        },
        iam::{IamMembershipRepository, OrganizationRepository, TeamRepository},
        notify::{
            connector::ConnectorAdapter,
            recipient::RecipientResolver,
            repositories::{
                NotifyConnectorRepository, NotifyDeliveryRepository, NotifyEventRepository,
                NotifyPolicyRepository, NotifyRouteReferenceRepository, NotifyTemplateRepository,
                OrganizationNotifyDefaultRepository, TeamNotifyDefaultRepository,
                UserNotifyEndpointRepository, UserNotifyPreferenceRepository,
            },
        },
    },
    infra::{
        notify::adapters::{
            EmailSmtpConnectorAdapter, LarkAppConnectorAdapter, LarkWebhookConnectorAdapter,
            SlackAppConnectorAdapter, SlackWebhookConnectorAdapter, WebhookConnectorAdapter,
        },
        persistence::repositories::{
            alert_rules::evaluation_state::PgAlertRuleEvalStateRepository,
            incidents::groups::PgIncidentGroupRepository,
            mute_rules::PgMuteRuleRepository,
            notify::{
                NotifyTemplateManagementRepository, PgNotifyConnectorRepository,
                PgNotifyDeliveryRepository, PgNotifyEventRepository, PgNotifyPolicyRepository,
                PgNotifyRouteReferenceRepository, PgNotifyTemplateRepository,
                PgOrganizationNotifyDefaultRepository, PgTeamNotifyDefaultRepository,
                PgUserNotifyEndpointRepository, PgUserNotifyPreferenceRepository,
            },
            semantic_groups::PgSemanticGroupRepository,
        },
    },
    shared::Result,
};

pub(super) struct AlertingRuntime {
    pub(super) alerting: Arc<AlertingService>,
    pub(super) notify: Arc<NotifyService>,
    pub(super) notify_engine: Arc<NotifyEngine>,
    pub(super) dashboard: Arc<DashboardService>,
    pub(super) notify_templates: Arc<dyn NotifyTemplateManagementRepository>,
    pub(super) mute_rules: Arc<dyn MuteRuleRepository>,
    pub(super) incident_groups: Arc<dyn IncidentGroupRepository>,
    pub(super) semantic_groups: Arc<dyn SemanticGroupRepository>,
}

impl AlertingRuntime {
    pub(super) fn build(
        settings: &Settings,
        core: &Core,
        query: &QueryRuntime,
        dashboard_contracts: Arc<dyn DashboardContractResolver>,
    ) -> Result<Self> {
        let eval_state: Arc<
            dyn crate::domain::alerting::repositories::AlertRuleEvalStateRepository,
        > = Arc::new(PgAlertRuleEvalStateRepository::new(core.pool.clone()));
        let template_repository = Arc::new(PgNotifyTemplateRepository::new(core.pool.clone()));
        let notify_templates: Arc<dyn NotifyTemplateManagementRepository> =
            template_repository.clone();
        let mute_rules: Arc<dyn MuteRuleRepository> =
            Arc::new(PgMuteRuleRepository::new(core.pool.clone()));
        let connector_adapters: Vec<Arc<dyn ConnectorAdapter>> = vec![
            Arc::new(EmailSmtpConnectorAdapter::new()),
            Arc::new(SlackAppConnectorAdapter::new()),
            Arc::new(SlackWebhookConnectorAdapter::new()),
            Arc::new(LarkAppConnectorAdapter::new()),
            Arc::new(LarkWebhookConnectorAdapter::new()),
            Arc::new(WebhookConnectorAdapter::new()),
        ];
        let connector_registry = Arc::new(ConnectorRegistry::new(connector_adapters)?);
        let connectors: Arc<dyn NotifyConnectorRepository> = Arc::new(
            PgNotifyConnectorRepository::new(core.pool.clone(), core.cipher_root_key.clone()),
        );
        let endpoints: Arc<dyn UserNotifyEndpointRepository> =
            Arc::new(PgUserNotifyEndpointRepository::new(core.pool.clone()));
        let preferences: Arc<dyn UserNotifyPreferenceRepository> =
            Arc::new(PgUserNotifyPreferenceRepository::new(core.pool.clone()));
        let notify_deliveries: Arc<dyn NotifyDeliveryRepository> =
            Arc::new(PgNotifyDeliveryRepository::new(core.pool.clone()));
        let notify_events: Arc<dyn NotifyEventRepository> =
            Arc::new(PgNotifyEventRepository::new(core.pool.clone()));
        let policies: Arc<dyn NotifyPolicyRepository> =
            Arc::new(PgNotifyPolicyRepository::new(core.pool.clone()));
        let team_defaults: Arc<dyn TeamNotifyDefaultRepository> =
            Arc::new(PgTeamNotifyDefaultRepository::new(core.pool.clone()));
        let organization_defaults: Arc<dyn OrganizationNotifyDefaultRepository> = Arc::new(
            PgOrganizationNotifyDefaultRepository::new(core.pool.clone()),
        );
        let route_references: Arc<dyn NotifyRouteReferenceRepository> =
            Arc::new(PgNotifyRouteReferenceRepository::new(core.pool.clone()));
        let memberships: Arc<dyn IamMembershipRepository> = core.iam_memberships.clone();
        let schedules: Arc<dyn crate::domain::alerting::repositories::ScheduleRepository> =
            core.schedules.clone();
        let teams: Arc<dyn TeamRepository> = core.teams.clone();
        let engine_templates: Arc<dyn NotifyTemplateRepository> = template_repository;
        let recipient_resolvers: Vec<Arc<dyn RecipientResolver>> = vec![
            Arc::new(FixedUsersResolver::new(memberships.clone())),
            Arc::new(CurrentOncallResolver::new(schedules.clone())),
            Arc::new(NextOncallResolver::new(schedules.clone())),
            Arc::new(ScheduleMembersResolver::new(schedules.clone())),
            Arc::new(TeamMembersResolver::new(teams.clone())),
            Arc::new(TeamLeadResolver::new(teams.clone())),
            Arc::new(EventUsersResolver::new(memberships.clone())),
            Arc::new(AlertOwnerResolver::new(memberships)),
        ];
        let resolver_registry = Arc::new(RecipientResolverRegistry::new(recipient_resolvers)?);
        let notify = Arc::new(NotifyService::new(
            connectors.clone(),
            endpoints.clone(),
            preferences.clone(),
            notify_deliveries.clone(),
            route_references,
            connector_registry.clone(),
        ));
        let notify_engine = Arc::new(NotifyEngine::new(NotifyEngineDependencies {
            connectors,
            endpoints,
            preferences,
            deliveries: notify_deliveries,
            events: notify_events,
            policies,
            team_defaults,
            organization_defaults,
            teams,
            templates: engine_templates,
            connector_registry,
            resolver_registry,
        }));
        let alerting = Arc::new(
            AlertingService::new(
                core.alert_rules.clone(),
                core.incidents.clone(),
                core.schedules.clone(),
                core.escalations.clone(),
            )
            .with_notify_engine(notify_engine.clone()),
        );
        let dispatcher = Arc::new(
            EscalationDispatcher::new(
                core.incidents.clone(),
                core.escalations.clone(),
                notify_engine.clone(),
            )
            .with_mute_rules(mute_rules.clone()),
        );
        let incident_groups: Arc<dyn IncidentGroupRepository> =
            Arc::new(PgIncidentGroupRepository::new(core.pool.clone()));
        let semantic_groups: Arc<dyn SemanticGroupRepository> =
            Arc::new(PgSemanticGroupRepository::new(core.pool.clone()));
        let evaluator = Arc::new(
            RuleEvaluator::new(
                core.alert_rules.clone(),
                core.incidents.clone(),
                eval_state,
                query.sql_engine.clone(),
                settings.alert_manager.eval_timeout_secs,
            )
            .with_grouping(incident_groups.clone(), semantic_groups.clone())
            .with_notify_engine(notify_engine.clone()),
        );
        let oncall_events = Arc::new(OncallEventProducer::new(notify_engine.clone(), schedules));
        let _alert_handles = core.roles.run_alert_manager.then(|| {
            crate::bootstrap::roles::alert_manager::spawn_alert_manager_loops(
                evaluator,
                dispatcher,
                core.orgs.clone() as Arc<dyn OrganizationRepository>,
                notify_engine.clone(),
                oncall_events,
                settings.alert_manager.clone(),
            )
        });
        let dashboard = Arc::new(
            DashboardService::new(core.dashboards.clone(), core.folders.clone())
                .with_draft_repository(core.dashboard_drafts.clone())
                .with_contract_resolver(dashboard_contracts),
        );

        Ok(Self {
            alerting,
            notify,
            notify_engine,
            dashboard,
            notify_templates,
            mute_rules,
            incident_groups,
            semantic_groups,
        })
    }
}
