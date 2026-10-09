// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! alert 角色：
//!
//! 同时跑两个 `tokio::interval`：
//! - evaluator tick（`eval_interval_secs`）：[`RuleEvaluator::tick`] 评估本节点拥有的
//!   enabled 规则，按 `for_periods` 累计 + open/resolve incident。
//! - dispatcher tick（`dispatch_interval_secs`）：[`EscalationDispatcher::tick`] 推进
//!   本节点拥有的 open incident 的 escalation step；同一轮里再按组织生产值班事件、投递通知。
//!
//! 多个 alert 节点按规则分担评估与升级（评估器和派发器各自持有 [`WorkOwnership`]）；通知投递与
//! 值班事件靠数据库认领 / 稳定事件 ID 去重，每个节点都跑。
//!
//! 在 standalone 模式下，bootstrap 阶段 spawn 后台任务；本 role 派发函数仅 idle。

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    app::{
        alerting::{EscalationDispatcher, RuleEvaluator},
        cluster::WorkOwnership,
        notify::{NotifyEngine, OncallEventProducer},
    },
    config::AlertSettings,
    domain::iam::OrganizationRepository,
    shared::time::TimestampMicros,
};

/// 归属指标里升级派发这项工作的名字。
const ESCALATION_WORK: &str = "escalation_dispatch";

pub fn spawn_alert_loops(
    evaluator: Arc<RuleEvaluator>,
    dispatcher: Arc<EscalationDispatcher>,
    orgs: Arc<dyn OrganizationRepository>,
    notify_engine: Arc<NotifyEngine>,
    oncall_events: Arc<OncallEventProducer>,
    ownership: Arc<WorkOwnership>,
    settings: AlertSettings,
) -> Vec<JoinHandle<()>> {
    let eval_interval = Duration::from_secs(settings.eval_interval_secs.max(1) as u64);
    let dispatch_interval = Duration::from_secs(settings.dispatch_interval_secs.max(1) as u64);

    let eval_handle = {
        let evaluator = evaluator.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(eval_interval);
            ticker.tick().await; // skip 首次（与 intake/compactor 一致）
            loop {
                ticker.tick().await;
                if let Err(e) = evaluator.tick(TimestampMicros::now()).await {
                    tracing::warn!(error = %e, "alert evaluator tick failed");
                }
            }
        })
    };

    let dispatch_handle = {
        let dispatcher = dispatcher.clone();
        let ownership = ownership.clone();
        let orgs = orgs.clone();
        let notify_engine = notify_engine.clone();
        let oncall_events = oncall_events.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(dispatch_interval);
            ticker.tick().await;
            loop {
                ticker.tick().await;
                match orgs.list().await {
                    Ok(org_list) => {
                        let mut escalated = 0;
                        for org in org_list {
                            let now = TimestampMicros::now();
                            match dispatcher.tick(&org.id, now).await {
                                Ok(owned) => escalated += owned,
                                Err(e) => {
                                    tracing::warn!(org=%org.id, error=%e, "dispatcher tick failed");
                                }
                            }
                            if let Err(e) = oncall_events.tick(&org.id, now).await {
                                tracing::warn!(
                                    org=%org.id,
                                    error=%e,
                                    "on-call notify event producer tick failed"
                                );
                            }
                            if let Err(e) = notify_engine
                                .process_pending_events(&org.id, now, 100)
                                .await
                            {
                                tracing::warn!(
                                    org=%org.id,
                                    error=%e,
                                    "notify event delivery tick failed"
                                );
                            }
                            if let Err(e) = notify_engine
                                .process_due_escalations(&org.id, now, 100)
                                .await
                            {
                                tracing::warn!(
                                    org=%org.id,
                                    error=%e,
                                    "notify acknowledgement escalation tick failed"
                                );
                            }
                        }
                        ownership.record_owned(ESCALATION_WORK, escalated);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "alert dispatcher: orgs.list failed");
                    }
                }
            }
        })
    };

    vec![eval_handle, dispatch_handle]
}
