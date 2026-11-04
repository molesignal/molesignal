// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 状态页组件依赖的装配。
//!
//! 告警事件和拨测状态的变化同时交给状态页自动化与组件依赖；依赖在收到变化的节点上评估，
//! alert 主节点另外定期全量评估，把健康变化交给跟随它的组件和页面的自动化规则。

use std::sync::Arc;

use super::{alerting::AlertingRuntime, core::Core};
use crate::{
    app::{
        status_page::{
            DependencySignalQueue, DependencySyntheticSink, StatusPageService,
            StatusPageSyntheticTransitionSink,
        },
        synthetics::{SyntheticTransitionSink, SyntheticTransitionSinks},
    },
    bootstrap::workers::status_page_dependencies::StatusPageDependencyWorker,
};

/// 拨测状态变化先交给状态页自动化，再请求重新评估组件依赖。
pub(super) fn synthetic_transition_sink(
    status_pages: Arc<StatusPageService>,
    signals: Arc<DependencySignalQueue>,
) -> Arc<dyn SyntheticTransitionSink> {
    Arc::new(SyntheticTransitionSinks::new(vec![
        Arc::new(StatusPageSyntheticTransitionSink::new(status_pages)),
        Arc::new(DependencySyntheticSink::new(signals)),
    ]))
}

/// 起评估循环。
pub(super) fn spawn_worker(core: &Core, alerting: &AlertingRuntime) {
    let _worker = StatusPageDependencyWorker::new(
        alerting.status_pages.clone(),
        alerting.dependency_signals.clone(),
    )
    .with_ownership(core.alert_ownership.clone(), core.roles.run_alert)
    .spawn();
}
