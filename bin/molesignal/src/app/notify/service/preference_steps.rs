// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一个类别的个人接收顺序怎么写：高紧急度一组；告警和升级两类还可以有低紧急度一组。每组最多 20 步，
//! 各步的等待时间与通知方式一一对应，同一组里不重复。

use std::collections::HashSet;

use super::NotifyService;
use crate::{
    domain::notify::preference::{NotifyCategory, validate_step_delays},
    shared::{Error, Result, ids::Id},
};

/// 一组最多几步。
const MAX_STEPS: usize = 20;

/// 保存时给的两组步骤。`*_step_delays_secs` 空着表示都不等（只做失败兜底）。
#[derive(Debug, Clone, Default)]
pub struct PreferenceSteps {
    pub endpoint_ids: Vec<Id>,
    pub step_delays_secs: Vec<i32>,
    pub low_endpoint_ids: Vec<Id>,
    pub low_step_delays_secs: Vec<i32>,
}

impl NotifyService {
    /// 一组步骤说得通：返回 `(通知方式, 等待秒数)`，按顺序。
    pub(super) async fn checked_steps(
        &self,
        organization_id: &Id,
        user_id: &Id,
        category: NotifyCategory,
        endpoint_ids: Vec<Id>,
        step_delays_secs: Vec<i32>,
    ) -> Result<Vec<(Id, i32)>> {
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

        let mut seen = HashSet::new();

        for endpoint_id in &endpoint_ids {
            if !seen.insert(endpoint_id.0.clone()) {
                return Err(Error::invalid(
                    "notify preference endpoint_ids must be unique",
                ));
            }

            self.endpoints
                .get(organization_id, user_id, endpoint_id)
                .await?;
        }

        Ok(endpoint_ids.into_iter().zip(step_delays_secs).collect())
    }
}
