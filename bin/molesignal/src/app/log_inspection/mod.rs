// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 定时日志巡检的应用层：保存时的校验（[`LogInspectionService`]）、周期的计算，以及一次执行
//! 的全过程（[`InspectionRunner`]：执行身份 → 模型 → 报告与结论等级 → 通知）。
//!
//! 调模型和调工具要用到模型配置与工具运行时，这一层只定义端口（[`InspectionAgent`]），实现和
//! 按周期触发的循环都在 `bootstrap::workers::log_inspection`。

use std::time::Duration;

mod agent;
mod identity;
mod input;
mod notification;
mod prompt;
mod runner;
mod schedule;
mod service;
#[cfg(test)]
pub(crate) mod test_support;
mod verdict;

pub use agent::{AgentAnswer, AgentFailure, AgentRequest, InspectionAgent, ToolCallLog};
pub use identity::{IamRunIdentity, RunIdentity};
pub use input::InspectionInput;
pub use notification::{ConnectorSender, NotificationSender};
pub use runner::{InspectionRunner, RunOutcome};
pub use schedule::{next_run_after, parse_timezone};
pub use service::LogInspectionService;

use crate::config::LogInspectionSettings;

/// 运行租约十分钟到期。总超时最长八分钟，再留两分钟判定「执行它的节点已经退出」，一条卡住
/// 的执行就一定在别的节点能重新认领之前被了结。
const MAX_RUN_TIMEOUT_SECS: u64 = 480;
const MIN_RUN_TIMEOUT_SECS: u64 = 30;

/// 实例对定时巡检的限制，来自 `[log_inspection]`。每次巡检都要调用模型，这些数限定它能用掉
/// 多少。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InspectionLimits {
    /// 同一条巡检两次执行之间的最短间隔。
    pub min_interval_secs: u64,
    /// 每个组织最多保存多少条。
    pub max_per_org: u32,
    /// 单节点同时执行多少条。
    pub concurrent_runs: usize,
    /// 单次巡检的总超时，含全部模型调用与查询。
    pub run_timeout: Duration,
    /// 报告保留多久。
    pub report_retention: Duration,
}

impl InspectionLimits {
    pub fn new(
        min_interval_secs: u64,
        max_per_org: u32,
        concurrent_runs: u32,
        run_timeout_secs: u64,
        report_retention_days: u32,
    ) -> Self {
        let run_timeout_secs = run_timeout_secs.clamp(MIN_RUN_TIMEOUT_SECS, MAX_RUN_TIMEOUT_SECS);
        let retention_days = u64::from(report_retention_days.max(1));

        Self {
            min_interval_secs: min_interval_secs.max(60),
            max_per_org,
            concurrent_runs: usize::try_from(concurrent_runs.max(1)).unwrap_or(1),
            run_timeout: Duration::from_secs(run_timeout_secs),
            report_retention: Duration::from_secs(retention_days * 24 * 3600),
        }
    }

    pub fn from_settings(settings: &LogInspectionSettings) -> Self {
        Self::new(
            settings.min_interval_secs,
            settings.max_per_org,
            settings.concurrent_runs,
            settings.run_timeout_secs,
            settings.report_retention_days,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_an_hour_twenty_inspections_and_five_minutes() {
        let limits = InspectionLimits::from_settings(&LogInspectionSettings::default());

        assert_eq!(limits.min_interval_secs, 3600);
        assert_eq!(limits.max_per_org, 20);
        assert_eq!(limits.concurrent_runs, 2);
        assert_eq!(limits.run_timeout, Duration::from_secs(300));
        assert_eq!(limits.report_retention, Duration::from_secs(30 * 24 * 3600));
    }

    #[test]
    fn a_run_always_ends_before_its_claim_can_be_taken_over() {
        // 运行租约是十分钟：再大的配置也收敛到八分钟。
        assert_eq!(
            InspectionLimits::new(3600, 20, 2, 3600, 30).run_timeout,
            Duration::from_secs(MAX_RUN_TIMEOUT_SECS)
        );
        assert!(
            Duration::from_secs(MAX_RUN_TIMEOUT_SECS) < crate::app::cluster::gate::DEFAULT_LEASE
        );
    }

    #[test]
    fn zeroes_fall_back_to_the_smallest_usable_values() {
        let limits = InspectionLimits::new(0, 0, 0, 0, 0);

        assert_eq!(limits.min_interval_secs, 60);
        assert_eq!(limits.max_per_org, 0);
        assert_eq!(limits.concurrent_runs, 1);
        assert_eq!(
            limits.run_timeout,
            Duration::from_secs(MIN_RUN_TIMEOUT_SECS)
        );
        assert_eq!(limits.report_retention, Duration::from_secs(24 * 3600));
    }
}
