// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一个类别的个人接收顺序怎么写：高紧急度一组；告警和升级两类还可以有低紧急度一组。每组最多 20 步，
//! 各步的等待时间与通知方式一一对应，同一组里不重复。
//!
//! A step may also ask the endpoint's IM app to page the person harder after the message (its
//! boost). The same endpoint may then appear in one group twice, once per boost — for example a
//! Feishu message first and a Feishu phone urgent five minutes later.

use std::collections::HashSet;

use domain::notify::preference::{NotifyCategory, StepBoost, validate_step_delays};
use kernel::{Error, Result, ids::Id};

use super::NotifyService;

/// 一组最多几步。
const MAX_STEPS: usize = 20;

/// 保存时给的两组步骤。`*_step_delays_secs` 空着表示都不等（只做失败兜底）；`*_step_boosts` 空着表示
/// 都不加急。
#[derive(Debug, Clone, Default)]
pub struct PreferenceSteps {
    pub endpoint_ids: Vec<Id>,
    pub step_delays_secs: Vec<i32>,
    pub step_boosts: Vec<Option<String>>,
    pub low_endpoint_ids: Vec<Id>,
    pub low_step_delays_secs: Vec<i32>,
    pub low_step_boosts: Vec<Option<String>>,
}

/// One checked step: the endpoint, how long it waits, and its boost.
pub(super) type CheckedStep = (Id, i32, Option<StepBoost>);

impl NotifyService {
    /// 一组步骤说得通：返回 `(通知方式, 等待秒数, 加急)`，按顺序。
    pub(super) async fn checked_steps(
        &self,
        organization_id: &Id,
        user_id: &Id,
        category: NotifyCategory,
        endpoint_ids: Vec<Id>,
        step_delays_secs: Vec<i32>,
        step_boosts: Vec<Option<String>>,
    ) -> Result<Vec<CheckedStep>> {
        if endpoint_ids.len() > MAX_STEPS {
            return Err(Error::invalid(format!(
                "notify preference supports at most {MAX_STEPS} endpoints"
            )));
        }

        let step_delays_secs = if step_delays_secs.is_empty() {
            vec![0; endpoint_ids.len()]
        } else if step_delays_secs.len() == endpoint_ids.len() {
            step_delays_secs
        } else {
            return Err(Error::invalid(
                "notify preference step_delays_secs must give one delay per endpoint",
            ));
        };

        validate_step_delays(category, &step_delays_secs)?;

        let step_boosts = if step_boosts.is_empty() {
            vec![None; endpoint_ids.len()]
        } else if step_boosts.len() == endpoint_ids.len() {
            step_boosts
                .iter()
                .map(|boost| {
                    boost
                        .as_deref()
                        .filter(|boost| !boost.trim().is_empty())
                        .map(StepBoost::parse)
                        .transpose()
                })
                .collect::<Result<Vec<_>>>()?
        } else {
            return Err(Error::invalid(
                "notify preference step_boosts must give one boost (or null) per endpoint",
            ));
        };

        let mut seen = HashSet::new();

        for (endpoint_id, boost) in endpoint_ids.iter().zip(&step_boosts) {
            if !seen.insert((endpoint_id.0.clone(), *boost)) {
                return Err(Error::invalid(
                    "a notify endpoint appears in one group at most once per boost",
                ));
            }

            let endpoint = self
                .endpoints
                .get(organization_id, user_id, endpoint_id)
                .await?;

            if let Some(boost) = boost {
                self.check_boost(organization_id, &endpoint.connector_id, *boost)
                    .await?;
            }
        }

        Ok(endpoint_ids
            .into_iter()
            .zip(step_delays_secs)
            .zip(step_boosts)
            .map(|((endpoint_id, delay_secs), boost)| (endpoint_id, delay_secs, boost))
            .collect())
    }

    /// The endpoint's connector offers this boost.
    async fn check_boost(
        &self,
        organization_id: &Id,
        connector_id: &Id,
        boost: StepBoost,
    ) -> Result<()> {
        let connector = self.connectors.get(organization_id, connector_id).await?;
        let adapter = self.registry.get(&connector.connector_type)?;

        if adapter.boosts().contains(&boost) {
            return Ok(());
        }

        Err(Error::invalid(format!(
            "{} cannot page with boost `{}`",
            connector.connector_type,
            boost.as_str()
        )))
    }
}
