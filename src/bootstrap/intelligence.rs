// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Intelligence repository 与后台分析 worker 装配。

use std::sync::Arc;

use super::{core::Core, iam::IamRuntime, license::LicenseRuntime};
use crate::{
    domain::{
        alerting::repositories::IncidentRcaRepository,
        iam::{IamMembershipRepository, OrganizationRepository},
        query::SlowQueryRepository,
    },
    infra::persistence::repositories::{
        incidents::rca::PgIncidentRcaRepository,
        intelligence::{
            PgIntelligenceRepository,
            chat_archives::{ChatArchiveRepository, PgChatArchiveRepository},
            chats::{ChatRepository, PgChatRepository},
            model_providers::{ModelProviderRepository, PgModelProviderRepository},
            prompts::{AgentPromptRepository, PgAgentPromptRepository},
            tool_control::PgToolControlRepository,
            toolsets::{AgentToolsetRepository, PgAgentToolsetRepository},
        },
        slow_queries::PgSlowQueryRepository,
    },
    intelligence::{model::IntelligenceRepository, tool_control::ToolControlRepository},
};

pub(super) fn build_model_providers(core: &Core) -> Arc<dyn ModelProviderRepository> {
    Arc::new(PgModelProviderRepository::new(
        core.pool.clone(),
        core.cipher_root_key.clone(),
    ))
}

pub(super) struct IntelligenceRuntime {
    pub(super) chats: Arc<dyn ChatRepository>,
    pub(super) intelligence: Arc<dyn IntelligenceRepository>,
    pub(super) toolsets: Arc<dyn AgentToolsetRepository>,
    pub(super) tool_control: Arc<dyn ToolControlRepository>,
    pub(super) prompts: Arc<dyn AgentPromptRepository>,
    pub(super) chat_archives: Arc<dyn ChatArchiveRepository>,
    pub(super) incident_rca: Arc<dyn IncidentRcaRepository>,
    pub(super) slow_queries: Arc<dyn SlowQueryRepository>,
}

impl IntelligenceRuntime {
    pub(super) fn build(
        core: &Core,
        iam: &IamRuntime,
        license: &LicenseRuntime,
        model_providers: Arc<dyn ModelProviderRepository>,
    ) -> Self {
        let chats: Arc<dyn ChatRepository> = Arc::new(PgChatRepository::new(core.pool.clone()));
        let intelligence: Arc<dyn IntelligenceRepository> =
            Arc::new(PgIntelligenceRepository::new(core.pool.clone()));
        let toolsets: Arc<dyn AgentToolsetRepository> =
            Arc::new(PgAgentToolsetRepository::new(core.pool.clone()));
        let tool_control: Arc<dyn ToolControlRepository> = Arc::new(PgToolControlRepository::new(
            core.pool.clone(),
            core.cipher_root_key.clone(),
        ));
        let prompts: Arc<dyn AgentPromptRepository> =
            Arc::new(PgAgentPromptRepository::new(core.pool.clone()));
        let chat_archives: Arc<dyn ChatArchiveRepository> =
            Arc::new(PgChatArchiveRepository::new(core.pool.clone()));
        let incident_rca: Arc<dyn IncidentRcaRepository> =
            Arc::new(PgIncidentRcaRepository::new(core.pool.clone()));
        let slow_queries: Arc<dyn SlowQueryRepository> =
            Arc::new(PgSlowQueryRepository::new(core.pool.clone()));

        let _slow_query_analyzer = core.roles.run_alert_manager.then(|| {
            crate::bootstrap::workers::slow_query_analyzer::SlowQueryAnalyzer::new(
                core.orgs.clone() as Arc<dyn OrganizationRepository>,
                slow_queries.clone(),
                crate::bootstrap::workers::slow_query_analyzer::SlowQueryAnalyzerConfig::default(),
            )
            .spawn()
        });
        let rca_generator = Arc::new(crate::api::rca::RcaGenerator::new(
            model_providers,
            prompts.clone(),
            incident_rca.clone(),
            crate::api::rca::RcaGenConfig::default(),
        ));
        let _rca_sweeper = core.roles.run_alert_manager.then(|| {
            crate::bootstrap::workers::rca_sweeper::RcaSweeper::new(
                core.orgs.clone() as Arc<dyn OrganizationRepository>,
                core.iam_memberships.clone() as Arc<dyn IamMembershipRepository>,
                iam.user_preferences.clone(),
                core.incidents.clone(),
                license.license.clone(),
                rca_generator,
                crate::bootstrap::workers::rca_sweeper::RcaSweeperConfig::default(),
            )
            .spawn()
        });

        Self {
            chats,
            intelligence,
            toolsets,
            tool_control,
            prompts,
            chat_archives,
            incident_rca,
            slow_queries,
        }
    }
}
