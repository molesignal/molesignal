// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The alert rules of the host events that need someone at once: an OOM kill, a hung task, a soft
//! lockup, an uncorrected hardware error, a link that lost its carrier and a stuck transmit queue.
//! Each is a real-time rule, counted as the events arrive: one event in five minutes opens an
//! incident. They are made disabled, so that nobody is paged before choosing to be, and are
//! switched on one by one. A rule is found again by its label, so installing again only adds the
//! ones that are missing and never changes one a person has edited.

use std::collections::BTreeMap;

use domain::{
    alerting::{
        escalation::DEFAULT_POLICY_ID,
        incident::Severity,
        rule::{
            AlertQuery, AlertRule, AlertRuleKind, AlertTrigger, ComparisonOp, GuideQuery,
            RuleGuide, RuleState,
        },
    },
    query::{QueryLanguage, StreamHint},
    stream::StreamType,
};
use kernel::{ids::Id, time::TimestampMicros};

use super::{EVENTS_STREAM, TAG};

/// The label that says which template made a rule.
pub const TEMPLATE_LABEL: &str = "node_agent.template";

/// The window the events are counted over: five minutes.
const WINDOW_SECS: u32 = 300;

/// How long an open incident keeps quiet about the same events.
const SILENCE_SECS: u32 = 1_800;

/// A rule worth having on the host events, ready to make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostAlert {
    pub key: &'static str,
    pub name: &'static str,
    /// The events the rule counts.
    pub condition: &'static str,
    pub severity: Severity,
    /// What to check first, as Markdown.
    pub steps: &'static str,
}

pub const ALERTS: [HostAlert; 6] = [
    HostAlert {
        key: "oom_kill",
        name: "主机 · OOM kill",
        condition: r#""event.name" = 'oom'"#,
        severity: Severity::Error,
        steps: "1. 在事件的 body 里看 `victim`（被杀的进程与容器）和 `trigger`（触发回收的进程）。\n2. 对照「主机 · 概览」的内存面板：是整机内存不足，还是容器的内存上限（`mem_limit_pages`）太小。\n3. 同一容器反复 OOM 时调整它的 limit 或排查内存泄漏。",
    },
    HostAlert {
        key: "hungtask",
        name: "主机 · hung task",
        condition: r#""event.name" = 'hungtask'"#,
        severity: Severity::Error,
        steps: "1. body 的 `blocked_processes_stack` 是卡在 D 状态的进程栈，`hung_task_timeout_secs` 是阈值。\n2. 栈里有 IO 路径时对照磁盘吞吐面板与 iowait；有锁时看 `cpus_stack` 里持锁的 CPU。\n3. 同一主机反复出现时考虑隔离该节点。",
    },
    HostAlert {
        key: "softlockup",
        name: "主机 · soft lockup",
        condition: r#""event.name" = 'softlockup'"#,
        severity: Severity::Critical,
        steps: "1. body 的 `cpu` 是卡住的 CPU，`comm`/`pid` 是占着它的任务，`cpus_stack` 是各 CPU 的栈。\n2. soft lockup 意味着内核在这颗 CPU 上长时间不调度，常见于驱动或内核缺陷。\n3. 记下内核版本与栈，必要时摘除节点。",
    },
    HostAlert {
        key: "ras_uncorrected",
        name: "主机 · 不可纠正的硬件错误",
        condition: r#""event.name" = 'ras' AND json_text(body, 'type') LIKE 'Uncorrected%'"#,
        severity: Severity::Critical,
        steps: "1. body 的 `dev` 与 `event` 说明出错的部件（MCE、EDAC 内存、PCIe AER），`info` 是详情。\n2. 不可纠正的错误可能已经影响数据：尽快迁走这台主机上的负载。\n3. 把 `dev`、`info` 交给硬件维护方更换部件。",
    },
    HostAlert {
        key: "netdev_carrier_down",
        name: "主机 · 网卡链路断开",
        condition: r#""event.name" = 'netdev_events' AND json_text(body, 'linkstatus') = 'linkstatus_carrierdown'"#,
        severity: Severity::Error,
        steps: "1. body 的 `ifname` 是网卡，`driver`/`firmware_version` 是驱动与固件。\n2. 检查网线、交换机端口与 bond 成员状态；同一时间多台主机断开时多半是交换机。\n3. 链路恢复时会有一条 `linkstatus_carrierup` 事件。",
    },
    HostAlert {
        key: "netdev_txqueue_timeout",
        name: "主机 · 网卡发送队列超时",
        condition: r#""event.name" = 'netdev_txqueue_timeout'"#,
        severity: Severity::Warning,
        steps: "1. body 的 `device_name`、`queue_index` 是超时的网卡与队列，`driver_name` 是驱动。\n2. 发送队列超时会让网卡复位，对照网络错误与丢包面板。\n3. 反复出现时升级网卡驱动或固件。",
    },
];

impl HostAlert {
    /// How many of the events arrived in the window.
    pub fn statement(&self) -> String {
        format!(
            "SELECT count(*) FROM \"{EVENTS_STREAM}\" WHERE {}",
            self.condition
        )
    }

    /// The events themselves, for the incident's guide: where they happened first.
    fn events(&self) -> String {
        format!(
            r#"SELECT * FROM "{EVENTS_STREAM}" WHERE {} ORDER BY _timestamp DESC LIMIT 100"#,
            self.condition
        )
    }

    /// The rule the template makes, disabled: a real-time count of the events over five minutes,
    /// raised by one, with what to check first and the host dashboard beside it.
    pub fn rule(&self, org_id: &Id, dashboard: Id) -> AlertRule {
        let now = TimestampMicros::now();
        let stream = StreamHint {
            name: EVENTS_STREAM.into(),
            stream_type: StreamType::LOGS,
        };

        AlertRule {
            id: Id::new(),
            org_id: org_id.clone(),
            name: self.name.into(),
            description: format!("Node Agent 上报的事件：{}", self.condition),
            enabled: false,
            kind: AlertRuleKind::RealTime,
            query: AlertQuery {
                language: QueryLanguage::Sql,
                statement: self.statement(),
                period_secs: WINDOW_SECS,
                stream: Some(stream.clone()),
                apm: None,
                rum: None,
                group_by: Vec::new(),
                composite: None,
                forecast: None,
                outlier: None,
                heartbeat: None,
                delay_secs: None,
                discovery: None,
                home_cluster: None,
            },
            trigger: AlertTrigger {
                operator: ComparisonOp::Gt,
                threshold: 0.0,
                for_periods: 1,
                silence_secs: SILENCE_SECS,
                no_data: None,
                recovery: None,
                priority: None,
                urgency: None,
            },
            anomaly_params: None,
            thresholds: Vec::new(),
            severity: Some(self.severity),
            escalation_policy_id: Id::from_string(DEFAULT_POLICY_ID),
            labels: BTreeMap::from([
                (TEMPLATE_LABEL.to_string(), self.key.to_string()),
                ("source".to_string(), TAG.to_string()),
            ]),
            annotations: BTreeMap::new(),
            guide: RuleGuide {
                steps: self.steps.into(),
                queries: vec![GuideQuery {
                    name: "这段时间的事件".into(),
                    language: QueryLanguage::Sql,
                    stream: Some(stream),
                    statement: self.events(),
                }],
                dashboards: vec![dashboard],
            },
            last_eval_at: None,
            last_state: RuleState::Healthy,
            created_at: now,
            updated_at: now,
        }
    }
}

/// The rule of `rules` a template made, by its label.
pub fn made_by<'a>(rules: &'a [AlertRule], alert: &HostAlert) -> Option<&'a AlertRule> {
    rules
        .iter()
        .find(|rule| rule.labels.get(TEMPLATE_LABEL).map(String::as_str) == Some(alert.key))
}
