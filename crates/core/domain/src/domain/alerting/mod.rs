// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警上下文。
//!
//! 包含：
//! - [`rule`]          告警规则定义与评估
//! - [`anomaly`]       MAD-baseline anomaly detector（`AlertRule.kind = Anomaly` 的求值器）
//! - [`incident`]      告警事件的生命周期（open / ack / resolved）
//! - [`handling`]      人对事件的处理：暂缓、手动指定、合并、升级通知的轮次
//! - [`timeline`]      事件的时间线与备注
//! - [`incident_group`] 相关 incident 的聚合分组
//! - [`semantic_group`] 告警分组规则（Alertmanager group_by 风格）
//! - [`schedule`]      On-call 排班轮值与临时 override
//! - [`escalation`]    PagerDuty 风格的多级升级策略
//! - [`source`]        告警来源：外部系统发来的告警怎么接入
//!
//! 评估 → 触发 → 创建 Incident → 按 EscalationPolicy 派发 →
//! 生成 Notify 事件 → Policy 匹配、接收人解析与投递 → 等 ack/超时 → 升级。

pub mod anomaly;
pub mod calendar;
pub mod composite;
pub mod discovery;
pub mod escalation;
pub mod forecast;
pub mod handling;
pub mod heartbeat;
pub mod holiday;
pub mod importance;
pub mod incident;
pub mod incident_group;
pub mod inhibit;
pub mod meeting;
pub mod mute;
pub mod outlier;
pub mod postmortem;
pub mod priority;
pub mod realtime_snapshot;
pub mod remediation;
pub mod remediation_template;
pub mod repositories;
pub mod response;
pub mod routing;
pub mod rule;
pub mod rule_conditions;
pub mod schedule;
pub mod semantic_group;
pub mod source;
pub mod swap;
pub mod ticket;
pub mod time_window;
pub mod timeline;
pub mod urgency;
