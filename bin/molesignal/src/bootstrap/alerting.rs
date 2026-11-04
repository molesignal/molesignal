// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警、通知与 dashboard 运行时装配。

use std::{sync::Arc, time::Duration};

use super::{
    alerting_notify::{connector_registry, resolver_registry},
    alerting_response::{ResponseInputs, ResponseServices},
    core::Core,
    query::QueryRuntime,
};
use crate::{
    app::{
        alerting::{
            AlertIncidentLifecycleSink, AlertIncidentLifecycleSinks, AlertInsights,
            AlertingService, CalendarFeeds, EscalationDispatcher, IncidentActions,
            IncidentResponseService, MergedRecoverySink, PostmortemService, RelatedIncidents,
            RemediationService, ResponseLifecycleSink, RuleEvaluator, ShiftSwaps,
            TicketLifecycleSink, TicketService, followups,
            realtime::{RealtimeAlertWorker, RealtimeCoordinator},
        },
        dashboard::{DashboardService, contract_registry::DashboardContractResolver},
        notify::{
            NotifyEngine, NotifyEngineDependencies, NotifyService, OncallEventProducer,
            ReplyCallbacks,
        },
        status_page::{
            DependencyAlertSink, DependencySignalQueue, DependencySources,
            StatusPageAlertIncidentSink, StatusPageService,
        },
    },
    config::Settings,
    domain::{
        alerting::{
            discovery::DiscoveryRepository, incident_group::IncidentGroupRepository,
            mute::MuteRuleRepository, semantic_group::SemanticGroupRepository,
            timeline::IncidentTimelineRepository,
        },
        iam::{IamMembershipRepository, OrganizationRepository, TeamRepository},
        notify::{
            reply::NotifyReplyRepository,
            repositories::{
                NotifyCallRepository, NotifyConnectorRepository, NotifyDeliveryRepository,
                NotifyEventRepository, NotifyFollowUpRepository, NotifyPolicyRepository,
                NotifyRouteReferenceRepository, NotifyTemplateRepository,
                OrganizationNotifyDefaultRepository, TeamNotifyDefaultRepository,
                UserNotifyEndpointRepository, UserNotifyPreferenceRepository,
            },
        },
    },
    infra::{
        cluster::{
            realtime_counts::ClusterRealtimeCounts, realtime_federation::GrpcFederatedCounts,
        },
        notify::StatusPageNotifyAdapter,
        persistence::repositories::{
            agent::im::PgImRepository,
            alert_heartbeats::PgHeartbeatRepository,
            alert_routing::{PgRoutingRuleRepository, PgTeamPolicyRepository},
            alert_rules::evaluation_state::PgAlertRuleEvalStateRepository,
            discovery_findings::PgDiscoveryRepository,
            holiday_calendars::PgHolidayCalendarRepository,
            incident_importance::PgImportanceRepository,
            incidents::{
                groups::PgIncidentGroupRepository, timeline::PgIncidentTimelineRepository,
            },
            inhibit_rules::PgInhibitRuleRepository,
            instance_settings::PgInstanceSettingsRepository,
            mute_rules::PgMuteRuleRepository,
            notify::{
                NotifyTemplateManagementRepository, PgNotifyCallRepository,
                PgNotifyConnectorRepository, PgNotifyDeliveryRepository, PgNotifyEventRepository,
                PgNotifyFollowUpRepository, PgNotifyPolicyRepository, PgNotifyReplyRepository,
                PgNotifyRouteReferenceRepository, PgNotifyTemplateRepository,
                PgOrganizationNotifyDefaultRepository, PgTeamNotifyDefaultRepository,
                PgUserNotifyEndpointRepository, PgUserNotifyPreferenceRepository,
            },
            realtime_snapshots::PgRealtimeSnapshotRepository,
            rum_applications::PgRumApplicationRepository,
            rum_issues::PgRumIssueRepository,
            semantic_groups::PgSemanticGroupRepository,
            status_pages::PgStatusPageRepository,
            synthetics::PgSyntheticRepository,
        },
        status_page::{
            ConnectorSlackApps, DnsStatusPageDomainVerifier, StatusPageAdminEmailNotifier,
            StatusPageAssetCleaner,
        },
    },
    shared::Result,
};

pub(super) struct AlertingRuntime {
    pub(super) alerting: Arc<AlertingService>,
    pub(super) notify: Arc<NotifyService>,
    pub(super) notify_engine: Arc<NotifyEngine>,
    pub(super) dashboard: Arc<DashboardService>,
    pub(super) status_pages: Arc<StatusPageService>,
    /// 告警事件与拨测状态变化后，请求重新评估状态页组件依赖的队列。
    pub(super) dependency_signals: Arc<DependencySignalQueue>,
    pub(super) notify_templates: Arc<dyn NotifyTemplateManagementRepository>,
    pub(super) mute_rules: Arc<dyn MuteRuleRepository>,
    pub(super) inhibitions: Arc<crate::app::alerting::Inhibitions>,
    pub(super) incident_groups: Arc<dyn IncidentGroupRepository>,
    pub(super) semantic_groups: Arc<dyn SemanticGroupRepository>,
    pub(super) evaluator: Arc<RuleEvaluator>,
    /// Where APM rules read their figures; filled once APM's query service is built.
    pub(super) apm_alerts: Arc<crate::app::alerting::ApmAlertSlot>,
    /// Where RUM rules read their figures, and the preview of the rule editor.
    pub(super) rum_alerts: Arc<dyn crate::domain::rum::RumAlertSource>,
    /// 路由：新事件交给哪个团队、走哪条升级策略。
    pub(super) routing: Arc<crate::app::alerting::AlertRouting>,
    /// 人对事件的处理：转派、升级、暂缓、合并、备注、批量处理与时间线。
    pub(super) actions: Arc<IncidentActions>,
    /// 短信回复、电话按键的回调。
    pub(super) reply_callbacks: Arc<ReplyCallbacks>,
    /// 洞察里人的部分：响应和值班负担。
    pub(super) insights: Arc<AlertInsights>,
    /// 像这个事件的过去的事件，和它触发前后的变更。
    pub(super) related: Arc<RelatedIncidents>,
    /// 值班日历的订阅。
    pub(super) calendar: Arc<CalendarFeeds>,
    /// 换班申请。
    pub(super) swaps: Arc<ShiftSwaps>,
    /// 心跳规则的打卡地址与最后一次打卡。
    pub(super) heartbeats: Arc<dyn crate::domain::alerting::heartbeat::HeartbeatRepository>,
    /// 修复动作：定义、发起、执行与验证。
    pub(super) remediation: Arc<RemediationService>,
    /// 工单：Jira 与飞书项目。
    pub(super) tickets: Arc<TicketService>,
    /// 重大事件。
    pub(super) response: Arc<IncidentResponseService>,
    /// 内部复盘。
    pub(super) postmortems: Arc<PostmortemService>,
    /// 自动发现找到的问题。
    pub(super) discovery_findings: Arc<dyn DiscoveryRepository>,
    /// 节假日日历。
    pub(super) holiday_calendars: Arc<crate::app::alerting::HolidayCalendars>,
    /// 组织的优先级与紧急度档位。
    pub(super) importance: Arc<crate::app::alerting::ImportanceService>,
    /// 实时规则在本集群各节点的计数之和：联邦里的汇总集群来拉。
    pub(super) realtime_counts: Arc<dyn crate::app::alerting::realtime::RealtimeCounts>,
}

impl AlertingRuntime {
    pub(super) fn build(
        settings: &Settings,
        core: &Core,
        query: &QueryRuntime,
        dashboard_contracts: Arc<dyn DashboardContractResolver>,
        alert_pipelines: Arc<crate::app::alerting::event_pipelines::AlertPipelines>,
    ) -> Result<Self> {
        let eval_state: Arc<
            dyn crate::domain::alerting::repositories::AlertRuleEvalStateRepository,
        > = Arc::new(PgAlertRuleEvalStateRepository::new(core.pool.clone()));

        let template_repository = Arc::new(PgNotifyTemplateRepository::new(core.pool.clone()));
        let notify_templates: Arc<dyn NotifyTemplateManagementRepository> =
            template_repository.clone();
        let mute_rules: Arc<dyn MuteRuleRepository> =
            Arc::new(PgMuteRuleRepository::new(core.pool.clone()));

        let connector_registry =
            connector_registry(Arc::new(PgImRepository::new(core.pool.clone())))?;

        let connectors: Arc<dyn NotifyConnectorRepository> = Arc::new(
            PgNotifyConnectorRepository::new(core.pool.clone(), core.cipher_root_key.clone()),
        );

        let endpoints: Arc<dyn UserNotifyEndpointRepository> =
            Arc::new(PgUserNotifyEndpointRepository::new(core.pool.clone()));
        let preferences: Arc<dyn UserNotifyPreferenceRepository> =
            Arc::new(PgUserNotifyPreferenceRepository::new(core.pool.clone()));
        let notify_deliveries: Arc<dyn NotifyDeliveryRepository> =
            Arc::new(PgNotifyDeliveryRepository::new(core.pool.clone()));
        let incident_deliveries = notify_deliveries.clone();
        let notify_events: Arc<dyn NotifyEventRepository> =
            Arc::new(PgNotifyEventRepository::new(core.pool.clone()));
        let follow_ups: Arc<dyn NotifyFollowUpRepository> =
            Arc::new(PgNotifyFollowUpRepository::new(core.pool.clone()));
        let calls: Arc<dyn NotifyCallRepository> =
            Arc::new(PgNotifyCallRepository::new(core.pool.clone()));
        let replies: Arc<dyn NotifyReplyRepository> =
            Arc::new(PgNotifyReplyRepository::new(core.pool.clone()));
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

        let resolver_registry = resolver_registry(memberships, schedules.clone(), teams.clone())?;

        let notify = Arc::new(NotifyService::new(
            connectors.clone(),
            endpoints.clone(),
            preferences.clone(),
            notify_deliveries.clone(),
            route_references,
            engine_templates.clone(),
            connector_registry.clone(),
        ));

        // 组织的优先级与紧急度档位：开事件、升级、发通知都按它。
        let importance = crate::app::alerting::ImportanceService::new(Arc::new(
            PgImportanceRepository::new(core.pool.clone()),
        ));

        let notify_engine = Arc::new(
            NotifyEngine::new(NotifyEngineDependencies {
                connectors: connectors.clone(),
                endpoints,
                preferences: preferences.clone(),
                deliveries: notify_deliveries.clone(),
                events: notify_events,
                follow_ups,
                calls,
                policies,
                team_defaults,
                organization_defaults,
                teams,
                templates: engine_templates,
                connector_registry,
                resolver_registry,
            })
            .with_alert_context(core.alert_rules.clone(), settings.http.external_url.clone())
            .with_silences(mute_rules.clone(), core.incidents.clone())
            .with_replies(replies.clone())
            .with_importance(importance.clone())
            .with_alert_pipelines(alert_pipelines.clone()),
        );

        let reply_callbacks = Arc::new(ReplyCallbacks::new(
            core.cipher_root_key.derive_key(b"notify-reply-callback/v1"),
            replies,
            connectors.clone(),
        ));

        let dependency_signals = Arc::new(DependencySignalQueue::new());

        let status_pages = build_status_pages(
            settings,
            core,
            connectors,
            DependencySources {
                alert_rules: core.alert_rules.clone(),
                mute_rules: mute_rules.clone(),
                synthetics: Arc::new(PgSyntheticRepository::new(
                    core.pool.clone(),
                    core.cipher_root_key.clone(),
                )),
                signals: dependency_signals.clone(),
            },
        );

        let incident_timeline: Arc<dyn IncidentTimelineRepository> =
            Arc::new(PgIncidentTimelineRepository::new(core.pool.clone()));

        // 路由与相似事件读 APM 服务目录；服务目录直接读库，不等 APM 的查询服务。
        let apm_catalog: Arc<dyn crate::domain::apm::ApmCatalogRepository> =
            Arc::new(crate::infra::apm::PgApmRepository::new(core.pool.clone()));

        let ResponseServices {
            insights,
            related,
            calendar,
            swaps,
            remediation,
            tickets,
            response,
            postmortems,
        } = ResponseServices::build(
            settings,
            core,
            ResponseInputs {
                timeline: incident_timeline.clone(),
                deliveries: notify_deliveries,
                preferences,
                notify_engine: notify_engine.clone(),
                apm_catalog: apm_catalog.clone(),
            },
        );

        // 告警事件的变化先交给状态页自动化，再请求重新评估组件依赖；并入的事件自己恢复时记在主事件上；
        // 关联的工单跟着写评论、流转，新事件按自动规则建单、按自动规则宣布为重大事件。
        let lifecycle_sink: Arc<dyn AlertIncidentLifecycleSink> =
            Arc::new(AlertIncidentLifecycleSinks::new(vec![
                Arc::new(StatusPageAlertIncidentSink::new(
                    status_pages.clone(),
                    mute_rules.clone(),
                )),
                Arc::new(DependencyAlertSink::new(dependency_signals.clone())),
                Arc::new(MergedRecoverySink::new(
                    core.incidents.clone(),
                    incident_timeline.clone(),
                )),
                Arc::new(TicketLifecycleSink(tickets.clone())),
                Arc::new(ResponseLifecycleSink(response.clone())),
            ]));

        let alerting = Arc::new(
            AlertingService::new(
                core.alert_rules.clone(),
                core.incidents.clone(),
                core.schedules.clone(),
                core.escalations.clone(),
            )
            .with_notify_engine(notify_engine.clone())
            .with_lifecycle_sink(lifecycle_sink.clone())
            .with_realtime_alerts(core.realtime_alerts.clone())
            .with_default_ack_timeout_secs(settings.alert.default_ack_timeout_secs)
            .with_timeline(incident_timeline.clone()),
        );

        let inhibitions = Arc::new(crate::app::alerting::Inhibitions::new(
            Arc::new(PgInhibitRuleRepository::new(core.pool.clone())),
            mute_rules.clone(),
            core.incidents.clone(),
            incident_timeline.clone(),
        ));

        let dispatcher = Arc::new(
            EscalationDispatcher::new(
                core.incidents.clone(),
                core.escalations.clone(),
                notify_engine.clone(),
            )
            .with_mute_rules(mute_rules.clone())
            .with_inhibitions(inhibitions.clone())
            .with_schedules(core.schedules.clone())
            .with_timeline(incident_timeline.clone())
            .with_ownership(core.alert_ownership.clone())
            .with_importance(importance.clone()),
        );

        let incident_groups: Arc<dyn IncidentGroupRepository> =
            Arc::new(PgIncidentGroupRepository::new(core.pool.clone()));
        let semantic_groups: Arc<dyn SemanticGroupRepository> =
            Arc::new(PgSemanticGroupRepository::new(core.pool.clone()));

        let apm_alerts = Arc::new(crate::app::alerting::ApmAlertSlot::default());
        let discovery_findings: Arc<dyn DiscoveryRepository> =
            Arc::new(PgDiscoveryRepository::new(core.pool.clone()));
        let holiday_calendars = crate::app::alerting::HolidayCalendars::new(Arc::new(
            PgHolidayCalendarRepository::new(core.pool.clone()),
        ));

        // A RUM rule reads the same rows as the RUM pages, through the engine the other rules use.
        let rum_alerts: Arc<dyn crate::domain::rum::RumAlertSource> =
            Arc::new(crate::infra::rum::alerts::RumAlerts::new(
                query.alert_engine.clone(),
                Arc::new(PgRumApplicationRepository::new(core.pool.clone())),
                Arc::new(PgRumIssueRepository::new(core.pool.clone())),
            ));

        // 路由读本集群的团队与 APM 服务目录。
        let routing = Arc::new(
            crate::app::alerting::AlertRouting::new(
                Arc::new(PgRoutingRuleRepository::new(core.pool.clone())),
                Arc::new(PgTeamPolicyRepository::new(core.pool.clone())),
                core.teams.clone(),
                core.escalations.clone(),
                Some(Arc::new(crate::app::alerting::routing::CatalogOwners(
                    apm_catalog,
                ))),
            )
            .with_importance(importance.clone()),
        );

        let actions = Arc::new(
            IncidentActions::new(
                core.incidents.clone(),
                incident_timeline.clone(),
                core.escalations.clone(),
                core.iam_memberships.clone(),
                routing.clone(),
                alerting.clone(),
            )
            .with_notify(notify_engine.clone(), incident_deliveries)
            .with_lifecycle_sink(lifecycle_sink.clone())
            .with_importance(importance.clone()),
        );

        let heartbeats: Arc<dyn crate::domain::alerting::heartbeat::HeartbeatRepository> =
            Arc::new(PgHeartbeatRepository::new(core.pool.clone()));

        // 一个进程跑全部角色时查询读得到自己的缓冲，规则不设评估延迟就不挪窗口；分角色集群里按
        // 实例设置的默认值挪，告警节点每分钟读一次。
        let roles = core.roles;
        let evaluation_delay = crate::app::alerting::rule_evaluator::DelaySettings::new(
            roles.run_intake && roles.run_querier && roles.run_alert && roles.run_compactor,
        );

        if roles.run_alert && !evaluation_delay.current().reads_own_buffer {
            evaluation_delay.spawn_refresh(Arc::new(PgInstanceSettingsRepository::new(
                core.pool.clone(),
            )));
        }

        let evaluator = Arc::new(
            RuleEvaluator::new(
                core.alert_rules.clone(),
                core.incidents.clone(),
                eval_state,
                query.alert_engine.clone(),
                settings.alert.eval_timeout_secs,
            )
            .with_delay(evaluation_delay)
            .with_grouping(incident_groups.clone(), semantic_groups.clone())
            .with_notify_engine(notify_engine.clone())
            .with_lifecycle_sink(lifecycle_sink)
            .with_apm(apm_alerts.clone())
            .with_rum(rum_alerts.clone())
            .with_discovery(discovery_findings.clone())
            .with_router(routing.router())
            .with_inhibitions(inhibitions.clone())
            .with_alert_pipelines(alert_pipelines)
            .with_timeline(incident_timeline.clone())
            .with_heartbeats(heartbeats.clone())
            .with_merger(actions.clone())
            // 多个 alert 节点按规则分担评估；同一规则两次评估相隔不足半个评估间隔视为同一周期。
            .with_ownership(core.alert_ownership.clone())
            .with_cycle_guard(Duration::from_millis(
                u64::from(settings.alert.eval_interval_secs.max(1)) * 500,
            )),
        );

        let oncall_events = Arc::new(OncallEventProducer::new(notify_engine.clone(), schedules));

        // 修复动作到点验证、工单定时同步：在告警节点上跑。
        let _followups = core.roles.run_alert.then(|| {
            followups::spawn(
                remediation.clone(),
                tickets.clone(),
                evaluator.clone(),
                core.alert_ownership.clone(),
            )
        });

        // 实时规则在接收写入的节点上计数：intake 与 standalone 角色运行匹配器的后台任务，每 5 秒存
        // 一份计数快照，重启时读回。
        let realtime_snapshots: Arc<
            dyn crate::domain::alerting::realtime_snapshot::RealtimeSnapshotRepository,
        > = Arc::new(PgRealtimeSnapshotRepository::new(core.pool.clone()));

        let _realtime_alert_worker = core.roles.run_intake.then(|| {
            RealtimeAlertWorker::new(core.realtime_alerts.clone(), core.alert_rules.clone())
                .with_snapshots(core.node_id.clone(), realtime_snapshots.clone())
                .spawn()
        });

        // 本集群各节点的计数之和：拉不到的节点用它最近的快照顶上。联邦里的汇总集群也来拉它。
        let realtime_counts: Arc<dyn crate::app::alerting::realtime::RealtimeCounts> = Arc::new(
            ClusterRealtimeCounts::new(
                core.registry.clone(),
                core.node_id.clone(),
                core.roles.run_intake.then(|| core.realtime_alerts.clone()),
                crate::app::self_telemetry::configured_cluster_token().map(Arc::<str>::from),
            )
            .with_snapshots(realtime_snapshots),
        );

        // 各节点的计数由负责这条规则的 alert 节点每秒拉一次、求和后开事件或解决；设了汇总集群的
        // 规则只在汇总集群判断，加上联邦里别的集群的计数。
        let _realtime_coordinator = core.roles.run_alert.then(|| {
            let federation = GrpcFederatedCounts::new(
                Arc::new(crate::app::alerting::sources::InstanceSettingsCluster(
                    Arc::new(PgInstanceSettingsRepository::new(core.pool.clone())),
                )),
                query.remote_clusters.clone(),
                query.cluster_org_link.clone(),
                query.cluster_secrets.clone(),
            );

            RealtimeCoordinator::new(
                core.alert_rules.clone(),
                evaluator.clone(),
                core.alert_ownership.clone(),
                realtime_counts.clone(),
            )
            .with_federation(Arc::new(federation))
            .spawn()
        });

        let _alert_handles = core.roles.run_alert.then(|| {
            crate::bootstrap::roles::alert::spawn_alert_loops(
                evaluator.clone(),
                dispatcher,
                core.orgs.clone() as Arc<dyn OrganizationRepository>,
                notify_engine.clone(),
                oncall_events,
                core.alert_ownership.clone(),
                settings.alert.clone(),
            )
        });

        let dashboard = Arc::new(
            DashboardService::new(core.dashboards.clone(), core.folders.clone())
                .with_draft_repository(core.dashboard_drafts.clone())
                .with_contract_resolver(dashboard_contracts),
        );

        let _status_page_notification_worker = core.roles.run_alert.then(|| {
            crate::bootstrap::workers::status_page_notifications::StatusPageNotificationWorker::new(
                status_pages.clone(),
            )
            .spawn()
        });

        let _status_page_maintenance_worker = core.roles.run_alert.then(|| {
            crate::bootstrap::workers::status_page_maintenance::StatusPageMaintenanceWorker::new(
                status_pages.clone(),
                Arc::new(StatusPageAssetCleaner::new(core.store.clone())),
            )
            .with_ownership(core.alert_ownership.clone())
            .spawn()
        });

        let _status_page_planned_maintenance_worker = core.roles.run_alert.then(|| {
            crate::bootstrap::workers::status_page_planned_maintenance::StatusPagePlannedMaintenanceWorker::new(
                status_pages.clone(),
            )
            .with_ownership(core.alert_ownership.clone())
            .spawn()
        });

        Ok(Self {
            alerting,
            notify,
            notify_engine,
            dashboard,
            status_pages,
            dependency_signals,
            notify_templates,
            mute_rules,
            inhibitions,
            incident_groups,
            semantic_groups,
            evaluator,
            apm_alerts,
            rum_alerts,
            routing,
            actions,
            reply_callbacks,
            insights,
            related,
            calendar,
            swaps,
            heartbeats,
            remediation,
            tickets,
            response,
            postmortems,
            discovery_findings,
            holiday_calendars,
            importance,
            realtime_counts,
        })
    }
}

fn build_status_pages(
    settings: &Settings,
    core: &Core,
    connectors: Arc<dyn NotifyConnectorRepository>,
    dependencies: DependencySources,
) -> Arc<StatusPageService> {
    let mut service = StatusPageService::new(
        Arc::new(PgStatusPageRepository::new(
            core.pool.clone(),
            core.cipher_root_key.clone(),
        )),
        core.incidents.clone(),
    )
    .with_notifications(
        Arc::new(StatusPageNotifyAdapter::new(core.email_sender.clone())),
        core.i18n_language.clone(),
        settings.http.external_url.clone(),
    )
    .with_managed_domain_tls(
        settings.http.tls.enabled
            && !settings.http.tls.account_email.trim().is_empty()
            && settings
                .http
                .external_url
                .trim()
                .to_ascii_lowercase()
                .starts_with("https://"),
    )
    .with_dependency_sources(dependencies)
    .with_slack_apps(Arc::new(ConnectorSlackApps::new(
        connectors,
        settings.http.external_url.clone(),
    )));

    match DnsStatusPageDomainVerifier::new(&settings.http.external_url) {
        Ok(verifier) => service = service.with_domain_verifier(Arc::new(verifier)),
        Err(error) => tracing::warn!(
            error = %error,
            "status-page custom domains are disabled until http.external_url is configured"
        ),
    }

    if let Some(email_sender) = core.email_sender.clone() {
        service = service.with_domain_health_notifier(Arc::new(StatusPageAdminEmailNotifier::new(
            core.pool.clone(),
            email_sender,
            core.i18n_language.clone(),
            settings.http.external_url.clone(),
        )));
    }

    Arc::new(service)
}
