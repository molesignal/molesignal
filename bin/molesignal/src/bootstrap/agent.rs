// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent repository 与后台分析 worker 装配。

use std::{path::PathBuf, sync::Arc};

use super::{core::Core, iam::IamRuntime};
use crate::{
    agent::{
        currencies::CurrencyRepository, feedback::AgentFeedbackRepository,
        inbound_mcp::InboundMcpRepository, knowledge::KnowledgeStore, memory::MemoryStore,
        model::AgentRepository, network::NetworkPolicyStore, runs::RunRepository,
        tool_control::ToolControlRepository,
    },
    app::{
        agent_usage::{AgentUsage, TraceReceiver, trace_queue},
        mcp::McpService,
        skills::{InstanceSkills, SkillService},
    },
    config::Settings,
    domain::{
        alerting::repositories::IncidentRcaRepository,
        iam::{IamMembershipRepository, OrganizationRepository},
        query::SlowQueryRepository,
    },
    infra::persistence::repositories::{
        agent::{
            PgAgentRepository,
            chat_archives::{ChatArchiveRepository, PgChatArchiveRepository},
            chats::{ChatRepository, PgChatRepository},
            currencies::PgCurrencyRepository,
            feedback::PgAgentFeedbackRepository,
            inbound_mcp::PgInboundMcpRepository,
            knowledge::PgKnowledgeStore,
            memory::PgMemoryStore,
            model_providers::{ModelProviderRepository, PgModelProviderRepository},
            network::PgNetworkPolicyStore,
            prompts::{AgentPromptRepository, PgAgentPromptRepository},
            runs::PgRunRepository,
            skills::PgSkillStore,
            tool_control::PgToolControlRepository,
            toolsets::{AgentToolsetRepository, PgAgentToolsetRepository},
            usage::PgAgentUsageRepository,
        },
        incidents::rca::PgIncidentRcaRepository,
        slow_queries::PgSlowQueryRepository,
    },
};

pub(super) fn build_model_providers(core: &Core) -> Arc<dyn ModelProviderRepository> {
    Arc::new(PgModelProviderRepository::new(
        core.pool.clone(),
        core.cipher_root_key.clone(),
    ))
}

/// 用量与预算服务，连同模型调用记录队列的接收端。
///
/// 写入管道里的 LLM 函数也要记用量，所以它先于 intake 建好；接收端在 intake 建好后交给
/// [`crate::bootstrap::workers::agent_traces::AgentTraceWriter`]。
pub(super) fn build_agent_usage(
    core: &Core,
    settings: &Settings,
) -> (Arc<AgentUsage>, TraceReceiver) {
    let (queue, receiver) = trace_queue();

    let usage = Arc::new(AgentUsage::new(
        Arc::new(PgAgentUsageRepository::new(core.pool.clone())),
        &settings.agent,
        queue,
    ));

    (usage, receiver)
}

pub(super) struct AgentRuntime {
    pub(super) chats: Arc<dyn ChatRepository>,
    pub(super) agent: Arc<dyn AgentRepository>,
    pub(super) toolsets: Arc<dyn AgentToolsetRepository>,
    pub(super) tool_control: Arc<dyn ToolControlRepository>,
    pub(super) inbound_mcp: Arc<dyn InboundMcpRepository>,
    pub(super) inbound_mcp_request_state_key: [u8; 32],
    pub(super) prompts: Arc<dyn AgentPromptRepository>,
    pub(super) chat_archives: Arc<dyn ChatArchiveRepository>,
    pub(super) incident_rca: Arc<dyn IncidentRcaRepository>,
    pub(super) slow_queries: Arc<dyn SlowQueryRepository>,
    pub(super) feedback: Arc<dyn AgentFeedbackRepository>,
    pub(super) skills: Arc<SkillService>,
    pub(super) network: Arc<dyn NetworkPolicyStore>,
    pub(super) mcp: Arc<McpService>,
    pub(super) runs: Arc<dyn RunRepository>,
    pub(super) knowledge: Arc<dyn KnowledgeStore>,
    pub(super) memory: Arc<dyn MemoryStore>,
    pub(super) currencies: Arc<dyn CurrencyRepository>,
}

impl AgentRuntime {
    pub(super) fn build(
        core: &Core,
        iam: &IamRuntime,
        settings: &Settings,
        model_providers: Arc<dyn ModelProviderRepository>,
        usage: Arc<AgentUsage>,
    ) -> Self {
        let chats: Arc<dyn ChatRepository> = Arc::new(PgChatRepository::new(core.pool.clone()));
        let agent: Arc<dyn AgentRepository> = Arc::new(PgAgentRepository::new(core.pool.clone()));
        let toolsets: Arc<dyn AgentToolsetRepository> =
            Arc::new(PgAgentToolsetRepository::new(core.pool.clone()));

        let tool_control: Arc<dyn ToolControlRepository> = Arc::new(PgToolControlRepository::new(
            core.pool.clone(),
            core.cipher_root_key.clone(),
        ));

        let inbound_mcp: Arc<dyn InboundMcpRepository> =
            Arc::new(PgInboundMcpRepository::new(core.pool.clone()));

        let inbound_mcp_request_state_key = core
            .cipher_root_key
            .derive_key(b"inbound-mcp/request-state/v1");

        let prompts: Arc<dyn AgentPromptRepository> =
            Arc::new(PgAgentPromptRepository::new(core.pool.clone()));
        let chat_archives: Arc<dyn ChatArchiveRepository> =
            Arc::new(PgChatArchiveRepository::new(core.pool.clone()));
        let incident_rca: Arc<dyn IncidentRcaRepository> =
            Arc::new(PgIncidentRcaRepository::new(core.pool.clone()));
        let slow_queries: Arc<dyn SlowQueryRepository> =
            Arc::new(PgSlowQueryRepository::new(core.pool.clone()));

        let _slow_query_analyzer = core.roles.run_alert.then(|| {
            crate::bootstrap::workers::slow_query_analyzer::SlowQueryAnalyzer::new(
                core.orgs.clone() as Arc<dyn OrganizationRepository>,
                slow_queries.clone(),
                crate::bootstrap::workers::slow_query_analyzer::SlowQueryAnalyzerConfig::default(),
            )
            .with_ownership(core.alert_ownership.clone())
            .spawn()
        });

        let feedback: Arc<dyn AgentFeedbackRepository> =
            Arc::new(PgAgentFeedbackRepository::new(core.pool.clone()));

        let network: Arc<dyn NetworkPolicyStore> =
            Arc::new(PgNetworkPolicyStore::new(core.pool.clone()));

        let skills = Arc::new(SkillService::new(
            Arc::new(PgSkillStore::new(
                core.pool.clone(),
                core.cipher_root_key.clone(),
            )),
            core.store.clone(),
            Arc::new(InstanceSkills::new(
                settings
                    .agent
                    .skill_paths
                    .iter()
                    .map(PathBuf::from)
                    .collect(),
            )),
            network.clone(),
            core.teams.clone(),
        ));

        // 实例目录每个节点各扫各的；仓库的自动同步由告警角色的节点去做。
        let _skill_paths =
            crate::bootstrap::workers::agent_skills::SkillPathScanner::new(skills.clone()).spawn();

        let _skill_repositories = core.roles.run_alert.then(|| {
            crate::bootstrap::workers::agent_skills::SkillRepositorySync::new(skills.clone())
                .spawn()
        });

        let mcp = Arc::new(
            McpService::new(tool_control.clone(), network.clone())
                .with_stdio_commands(settings.agent.mcp_stdio_commands.clone()),
        );

        // 外部 MCP 服务器的定期核对由告警角色的节点去做。
        let _mcp_resync = core
            .roles
            .run_alert
            .then(|| crate::bootstrap::workers::agent_mcp::McpResync::new(mcp.clone()).spawn());

        // 排队与事件的提醒：每个节点都听，执行 Run 与续读事件的节点都靠它被叫醒。
        let runs = Arc::new(PgRunRepository::new(core.pool.clone()));
        let _run_notifications = runs
            .clone()
            .spawn_notification_listener(settings.meta_database.dsn.clone());

        let rca_generator = Arc::new(crate::api::rca::RcaGenerator::new(
            model_providers,
            prompts.clone(),
            incident_rca.clone(),
            usage,
            crate::api::rca::RcaGenConfig::default(),
        ));

        let _rca_sweeper = core.roles.run_alert.then(|| {
            crate::bootstrap::workers::rca_sweeper::RcaSweeper::new(
                core.orgs.clone() as Arc<dyn OrganizationRepository>,
                core.iam_memberships.clone() as Arc<dyn IamMembershipRepository>,
                iam.user_preferences.clone(),
                core.incidents.clone(),
                rca_generator,
                crate::bootstrap::workers::rca_sweeper::RcaSweeperConfig::default(),
            )
            .with_ownership(core.alert_ownership.clone())
            .spawn()
        });

        Self {
            chats,
            agent,
            toolsets,
            tool_control,
            inbound_mcp,
            inbound_mcp_request_state_key,
            prompts,
            chat_archives,
            incident_rca,
            slow_queries,
            feedback,
            skills,
            network,
            mcp,
            runs,
            knowledge: Arc::new(PgKnowledgeStore::new(core.pool.clone())),
            memory: Arc::new(PgMemoryStore::new(core.pool.clone())),
            currencies: Arc::new(PgCurrencyRepository::new(core.pool.clone())),
        }
    }
}
