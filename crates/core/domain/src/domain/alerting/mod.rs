// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警上下文。
//!
//! 包含：
//! - [`rule`]          告警规则定义与评估
//! - [`anomaly`]       MAD-baseline anomaly detector（`AlertRule.kind = Anomaly` 的求值器）
//! - [`incident`]      告警事件的生命周期（open / ack / resolved）
//! - [`incident_group`] 相关 incident 的聚合分组
//! - [`semantic_group`] 告警分组规则（Alertmanager group_by 风格）
//! - [`schedule`]      On-call 排班轮值与临时 override
//! - [`escalation`]    PagerDuty 风格的多级升级策略
//!
//! 评估 → 触发 → 创建 Incident → 按 EscalationPolicy 派发 →
//! 生成 Notify 事件 → Policy 匹配、接收人解析与投递 → 等 ack/超时 → 升级。

pub mod anomaly;
pub mod escalation;
pub mod incident;
pub mod incident_group;
pub mod mute;
pub mod repositories;
pub mod rule;
pub mod schedule;
pub mod semantic_group;
