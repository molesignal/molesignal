// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Prometheus 写入的活跃 series 上限：组织与指标的上限是平台设置，每个节点启动时读一次，之后
//! 每隔半分钟再读；改它的节点在保存时立即换上（见 `routes/instance/data.rs`）。读不出时保持
//! 当前的上限。

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    domain::iam::InstanceSettingsRepository, infra::intake::PrometheusSeriesAdmission,
    shared::drain::DrainController,
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(30);

pub fn spawn(
    admission: Arc<PrometheusSeriesAdmission>,
    instance_settings: Arc<dyn InstanceSettingsRepository>,
    drain: Arc<DrainController>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        // 第一次立即触发：启动时就换上平台设置里的上限。
        let mut ticker = tokio::time::interval(REFRESH_INTERVAL);

        loop {
            ticker.tick().await;

            if drain.is_draining() {
                break;
            }

            match instance_settings.get().await {
                Ok(settings) => admission.set_limits(
                    settings.max_active_series_per_org as usize,
                    settings.max_active_series_per_metric as usize,
                ),
                Err(error) => {
                    tracing::warn!(%error, "series limits unreadable; keeping the current limits");
                }
            }
        }
    })
}
