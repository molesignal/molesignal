// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 根因分析（RCA）sweeper：周期遍历各 org 的活跃 incident（open + acknowledged），
//! 对尚无 RCA 的，调 LLM 产出根因摘要写回 `incident_rca`。生成逻辑复用
//! [`crate::api::rca::RcaGenerator`]（与 HTTP 按需触发同源，不漂移）。
//!
//! 单点周期任务：每个 alert 节点都起，但只有主节点（见 [`WorkOwnership::is_primary`]）执行，否则
//! 同一个 incident 会被多个节点各调一次模型。RCA 是 agent 能力 ——

//! 无可用 provider 的 org 直接跳过；已有 RCA 的 incident 不重复生成。失败仅 warn、下个
//! tick 自然重试（不落失败行）。

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    api::rca::{RcaGenerator, RcaOutputLocale},
    app::cluster::{PeerRole, WorkOwnership},
    domain::{
        alerting::repositories::IncidentRepository,
        iam::{IamMembershipRepository, OrganizationRepository},
    },
    infra::persistence::repositories::user_preferences::UserPreferencesRepository,
    shared::{Result, time::TimestampMicros},
};

#[derive(Debug, Clone)]
pub struct RcaSweeperConfig {
    pub interval_secs: u64,
    /// 每 tick 全局生成上限（成本护栏）。
    pub max_per_tick: usize,
}

impl Default for RcaSweeperConfig {
    fn default() -> Self {
        Self {
            interval_secs: 180,
            max_per_tick: 20,
        }
    }
}

pub struct RcaSweeper {
    orgs: Arc<dyn OrganizationRepository>,
    memberships: Arc<dyn IamMembershipRepository>,
    user_preferences: Arc<dyn UserPreferencesRepository>,
    incidents: Arc<dyn IncidentRepository>,

    generator: Arc<RcaGenerator>,
    cfg: RcaSweeperConfig,
    /// 只有 alert 主节点生成 RCA；默认不分担（本节点就是主节点）。
    ownership: Arc<WorkOwnership>,
}

impl RcaSweeper {
    pub fn new(
        orgs: Arc<dyn OrganizationRepository>,
        memberships: Arc<dyn IamMembershipRepository>,
        user_preferences: Arc<dyn UserPreferencesRepository>,
        incidents: Arc<dyn IncidentRepository>,

        generator: Arc<RcaGenerator>,
        cfg: RcaSweeperConfig,
    ) -> Self {
        Self {
            orgs,
            memberships,
            user_preferences,
            incidents,

            generator,
            cfg,
            ownership: WorkOwnership::solo(PeerRole::Alert),
        }
    }

    /// 与其余 alert 节点分担：只有主节点生成 RCA。
    pub fn with_ownership(mut self, ownership: Arc<WorkOwnership>) -> Self {
        self.ownership = ownership;
        self
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut tick =
                tokio::time::interval(Duration::from_secs(self.cfg.interval_secs.max(1)));
            tick.tick().await; // skip 首次（与其它周期任务一致）

            loop {
                tick.tick().await;

                if !self.ownership.is_primary().await {
                    continue;
                }

                if let Err(e) = self.sweep_once().await {
                    tracing::warn!(error = %e, "rca sweep failed");
                }
            }
        })
    }

    #[tracing::instrument(
        name = "worker.rca_sweeper",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "rca_sweeper")
    )]
    async fn sweep_once(&self) -> Result<()> {
        let now = TimestampMicros::now();
        let orgs = self.orgs.list().await?;
        let mut generated = 0usize;

        for org in orgs {
            if generated >= self.cfg.max_per_tick {
                break;
            }

            // 选 org 的可用 provider（enabled + key_set）；无则跳过该 org。
            let Some(provider) = self.generator.pick_provider_for(&org.id).await? else {
                continue;
            };

            let locale = self.preferred_locale_for_org(&org.id).await?;
            let actives = self.incidents.list_active(&org.id).await?;

            for incident in actives {
                if generated >= self.cfg.max_per_tick {
                    break;
                }

                // 仅对尚无 RCA 的 incident 生成。
                if self.generator.has_rca(&incident.id).await? {
                    continue;
                }

                match self
                    .generator
                    .generate_with_provider_for_locale(&org.id, &provider, &incident, now, locale)
                    .await
                {
                    Ok(_) => generated += 1,
                    Err(e) => {
                        tracing::warn!(
                            org = %org.id.0,
                            incident = %incident.id.0,
                            error = %e,
                            "rca generation failed"
                        );
                    }
                }
            }
        }

        if generated > 0 {
            tracing::info!(generated, "incident rca sweep generated summaries");
        }

        Ok(())
    }

    /// 后台任务没有当前 HTTP 用户，因此使用组织内最早加入成员的界面语言
    /// 作为该组织 RCA 的默认输出语言。角色不参与这项非授权决策。
    async fn preferred_locale_for_org(
        &self,
        org_id: &crate::shared::ids::Id,
    ) -> Result<RcaOutputLocale> {
        let mut memberships = self.memberships.list_for_org(org_id).await?;
        memberships.sort_by_key(|membership| membership.joined_at.0);

        let Some(primary_member) = memberships.first() else {
            return Ok(RcaOutputLocale::EnUs);
        };

        let preferences = self.user_preferences.get(&primary_member.user_id).await?;
        Ok(RcaOutputLocale::from_language_tag(&preferences.language))
    }
}
