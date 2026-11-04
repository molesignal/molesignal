// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一个人收不收得到告警：排班、升级策略、个人设置里标出来，升级时跳过谁都收不到的一级。
//!
//! 看告警和升级两类通知（`alert`、`escalation`）的个人设置：开着、至少有一步的通知方式启用且已
//! 验证、所在连接器启用且没出错，就是「收得到」。没有这样的一步，但所在团队或组织的默认路由能把
//! 这两类通知发到群里，是「只能靠群」。都没有是「收不到」。不看免打扰：那是一时的，不是没设好。

use std::collections::HashMap;

use serde::Serialize;

use super::NotifyEngine;
use crate::{
    domain::notify::{connector::ConnectorStatus, preference::NotifyCategory},
    shared::{Result, ids::Id},
};

/// 告警通知会用到的两类个人设置。
const CATEGORIES: [NotifyCategory; 2] = [NotifyCategory::Escalation, NotifyCategory::Alert];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reachability {
    /// 自己有能用的通知方式。
    Personal,
    /// 自己没有，但团队或组织的默认路由会发到群里。
    GroupOnly,
    /// 谁都收不到。
    None,
}

impl NotifyEngine {
    /// 这些人各自收不收得到告警，按给出的顺序，重复的只算一次。
    pub async fn reachability(
        &self,
        organization_id: &Id,
        user_ids: &[Id],
    ) -> Result<Vec<(Id, Reachability)>> {
        let org_group = self.organization_group_route(organization_id).await?;
        let mut seen: HashMap<Id, Reachability> = HashMap::new();
        let mut answers = Vec::new();

        for user_id in user_ids {
            if seen.contains_key(user_id) {
                continue;
            }

            let reach = if self.personally_reachable(organization_id, user_id).await? {
                Reachability::Personal
            } else if org_group || self.team_group_route(organization_id, user_id).await? {
                Reachability::GroupOnly
            } else {
                Reachability::None
            };

            seen.insert(user_id.clone(), reach);
            answers.push((user_id.clone(), reach));
        }

        Ok(answers)
    }

    async fn personally_reachable(&self, organization_id: &Id, user_id: &Id) -> Result<bool> {
        for category in CATEGORIES {
            let Some(preference) = self
                .preferences
                .get(organization_id, user_id, category)
                .await?
            else {
                continue;
            };

            if !preference.enabled {
                continue;
            }

            for step in &preference.steps {
                let Ok(endpoint) = self
                    .endpoints
                    .get(organization_id, user_id, &step.endpoint_id)
                    .await
                else {
                    continue;
                };

                if !endpoint.enabled || !endpoint.verified {
                    continue;
                }

                let Ok(connector) = self
                    .connectors
                    .get(organization_id, &endpoint.connector_id)
                    .await
                else {
                    continue;
                };

                if connector.enabled && connector.status != ConnectorStatus::Error {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    async fn organization_group_route(&self, organization_id: &Id) -> Result<bool> {
        for category in CATEGORIES {
            if self
                .organization_defaults
                .get(organization_id, category)
                .await?
                .is_some_and(|defaults| defaults.enabled && !defaults.routes.is_empty())
            {
                return Ok(true);
            }
        }

        Ok(false)
    }

    async fn team_group_route(&self, organization_id: &Id, user_id: &Id) -> Result<bool> {
        for team in self.teams.list_for_member(organization_id, user_id).await? {
            for category in CATEGORIES {
                if self
                    .team_defaults
                    .get(organization_id, &team.id, category)
                    .await?
                    .is_some_and(|defaults| defaults.enabled && !defaults.routes.is_empty())
                {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }
}
