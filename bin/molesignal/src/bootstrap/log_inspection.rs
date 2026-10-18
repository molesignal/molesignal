// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 定时日志巡检的装配。
//!
//! 保存、启停和读取报告的服务每个节点都有，由 HTTP 接口使用；按周期执行的循环只在 alert 角色
//! 的节点上起。两边共用同一个模型入口和同一条通知路径：保存时核对的模型配置和通知渠道，就是
//! 执行时用的那两样。

use std::sync::Arc;

use crate::{
    agent::tool_control::ToolControlRepository,
    app::{
        agent_usage::AgentUsage,
        iam::{IamAccessService, IamService},
        log_inspection::{
            ConnectorSender, IamRunIdentity, InspectionAgent, InspectionLimits, InspectionRunner,
            LogInspectionService, NotificationSender,
        },
        notify::NotifyService,
        tools::ToolRuntime,
    },
    bootstrap::workers::log_inspection::{LogInspectionWorker, agent::ModelAgent},
    config::Settings,
    domain::{i18n_language::I18nLanguageRepository, run_claim::RunGate, stream::StreamRepository},
    infra::persistence::repositories::{
        agent::model_providers::ModelProviderRepository, log_inspection::PgLogInspectionRepository,
    },
};

pub(super) struct LogInspectionDependencies {
    pub(super) pool: sqlx::PgPool,
    pub(super) streams: Arc<dyn StreamRepository>,
    pub(super) model_providers: Arc<dyn ModelProviderRepository>,
    /// 巡检的模型调用记进组织的每日用量，预算用完时不执行。
    pub(super) agent_usage: Arc<AgentUsage>,
    pub(super) tool_control: Arc<dyn ToolControlRepository>,
    pub(super) tools: Arc<ToolRuntime>,
    pub(super) notify: Arc<NotifyService>,
    pub(super) iam: Arc<IamService>,
    pub(super) iam_access: Arc<IamAccessService>,
    pub(super) i18n_language: Arc<dyn I18nLanguageRepository>,
    /// `Some`：本节点承担 alert 角色，按周期执行巡检，每条巡检开始前先过这个准入。
    pub(super) run_gate: Option<Arc<dyn RunGate>>,
}

/// 装好服务；本节点承担 alert 角色时顺带起后台循环。
pub(super) fn build(
    settings: &Settings,
    dependencies: LogInspectionDependencies,
) -> Arc<LogInspectionService> {
    let store = Arc::new(PgLogInspectionRepository::new(dependencies.pool));
    let limits = InspectionLimits::from_settings(&settings.log_inspection);

    let agent: Arc<dyn InspectionAgent> = Arc::new(ModelAgent::new(
        dependencies.model_providers,
        dependencies.tool_control,
        dependencies.tools,
        dependencies.agent_usage,
    ));

    let sender: Arc<dyn NotificationSender> = Arc::new(ConnectorSender::new(dependencies.notify));

    if let Some(run_gate) = dependencies.run_gate {
        let runner = Arc::new(InspectionRunner::new(
            store.clone(),
            store.clone(),
            Arc::new(IamRunIdentity::new(
                dependencies.iam,
                dependencies.iam_access,
            )),
            agent.clone(),
            sender.clone(),
            dependencies.i18n_language,
            settings.http.external_url.clone(),
            limits,
        ));

        // 进程存活期间一直运行，不需要留着句柄。
        let _worker = Arc::new(LogInspectionWorker::new(runner).with_gate(run_gate)).spawn();
    }

    Arc::new(LogInspectionService::new(
        store.clone(),
        store,
        dependencies.streams,
        sender,
        agent,
        limits,
    ))
}
