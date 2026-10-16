// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 业务服务的装配。
//!
//! 服务每个节点都有，由 HTTP 接口使用。告警事件和拨测状态的变化同时交给状态页自动化与业务服务；
//! 业务服务在收到变化的节点上评估，alert 主节点另外定期全量评估，把健康变化交给关联的状态页组件。

use std::sync::Arc;

use super::{alerting::AlertingRuntime, core::Core};
use crate::{
    app::{
        business_service::{
            BusinessServiceDependencies, BusinessServiceService, BusinessServiceSignalQueue,
            BusinessServiceSyntheticSink,
        },
        status_page::{StatusPageService, StatusPageSyntheticTransitionSink},
        synthetics::{SyntheticTransitionSink, SyntheticTransitionSinks},
    },
    bootstrap::workers::business_service_health::BusinessServiceHealthWorker,
    domain::synthetics::SyntheticRepository,
    infra::persistence::repositories::business_services::PgBusinessServiceRepository,
};

/// 拨测状态变化先交给状态页自动化，再请求重新评估业务服务。
pub(super) fn synthetic_transition_sink(
    status_pages: Arc<StatusPageService>,
    signals: Arc<BusinessServiceSignalQueue>,
) -> Arc<dyn SyntheticTransitionSink> {
    Arc::new(SyntheticTransitionSinks::new(vec![
        Arc::new(StatusPageSyntheticTransitionSink::new(status_pages)),
        Arc::new(BusinessServiceSyntheticSink::new(signals)),
    ]))
}

/// 装好服务并起评估循环。
pub(super) fn build(
    core: &Core,
    alerting: &AlertingRuntime,
    synthetics: Arc<dyn SyntheticRepository>,
) -> Arc<BusinessServiceService> {
    let service = Arc::new(BusinessServiceService::new(BusinessServiceDependencies {
        repository: Arc::new(PgBusinessServiceRepository::new(core.pool.clone())),
        incidents: core.incidents.clone(),
        alert_rules: core.alert_rules.clone(),
        mute_rules: alerting.mute_rules.clone(),
        synthetics,
        status_pages: alerting.status_pages.clone(),
        signals: alerting.business_service_signals.clone(),
    }));

    let _health_worker = BusinessServiceHealthWorker::new(service.clone())
        .with_ownership(core.alert_ownership.clone(), core.roles.run_alert)
        .spawn();

    service
}
