// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警来源：外部系统把告警发进来时的身份与配置。
//!
//! 每个来源一个 Token（`mspat-<36>`，与 API Token 同一个样子，但只能用来接入告警），来源的类型
//! 决定请求体按哪家的格式读。来源开的事件带标签 `alert.source = external` 与 `external.kind`，
//! `rule_id` 是来源的 id；指纹由来源 id 和去重键算出，不同来源用了同一个去重键也不会撞。
//!
//! 联邦的各个集群之间来源随配置同步（Token 的哈希一起同步，同一个 Token 在每个集群都能用），
//! 事件只在来源的接收集群（[`AlertSource::home_cluster`]）上开：别的集群收到它的告警转给接收集群。

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::incident::{EXTERNAL_KIND_LABEL, Severity};
use crate::{
    domain::iam::api_token::TOKEN_MARKER,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod alert;
mod email;
mod mailbox;
mod record;

pub use alert::{
    AlertMember, ExternalAction, ExternalAlert, MAX_ALERT_ANNOTATIONS, MAX_ALERT_LABELS,
    MAX_ANNOTATION_CHARS, MAX_DEDUP_KEY_CHARS, MAX_MEMBERS, MAX_SUMMARY_CHARS, MemberStatus,
    severity_from_word,
};
pub use email::{DEFAULT_RESOLVE_PATTERN, EmailRules, MAX_ALLOWED_SENDERS};
pub use mailbox::{
    DEFAULT_MAILBOX_POLL_SECS, MAX_MAILBOX_POLL_SECS, MIN_MAILBOX_POLL_SECS, MailCursor, Mailbox,
    MailboxAuth, MailboxRepository, MailboxSecurity, MailboxSettings,
};
pub use record::{
    AlertOutcome, IntakeAlert, IntakeRequest, IntakeRequestRepository, IntakeStatus, KEPT_REQUESTS,
    KEPT_REQUESTS_MICROS, KeyLock, MAX_BODY_EXCERPT_BYTES, MAX_RECORDED_ALERTS, SourceIncident,
    SourceIncidentRepository,
};

/// 来源名称最多几个字符。
pub const MAX_SOURCE_NAME_CHARS: usize = 128;
/// 来源上最多几个附加标签。
pub const MAX_SOURCE_LABELS: usize = 32;
/// 标签键最多几个字符。
pub const MAX_LABEL_KEY_CHARS: usize = 128;
/// 标签值最多几个字符。
pub const MAX_LABEL_VALUE_CHARS: usize = 1024;

/// 来源的类型：请求体按哪家的格式读。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertSourceKind {
    /// MoleSignal 自己的接口：触发、确认、恢复。
    Api,
    Alertmanager,
    Grafana,
    Datadog,
    Pagerduty,
    /// 阿里云云监控：阈值报警回调与事件订阅推送。
    AliyunCms,
    /// 腾讯云可观测平台（原云监控）：通知模板里的接口回调。
    TencentMonitor,
    /// Amazon CloudWatch：告警发到 SNS 主题，主题的 HTTPS 订阅推过来。
    AwsCloudwatch,
    /// Zabbix：Webhook 媒介类型发来的问题、恢复与更新。
    Zabbix,
    /// 邮件：读一个 IMAP 邮箱，或由已有的邮件系统把原始邮件 POST 过来。
    Email,
}

impl AlertSourceKind {
    pub const ALL: [Self; 10] = [
        Self::Api,
        Self::Alertmanager,
        Self::Grafana,
        Self::Datadog,
        Self::Pagerduty,
        Self::AliyunCms,
        Self::TencentMonitor,
        Self::AwsCloudwatch,
        Self::Zabbix,
        Self::Email,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Api => "api",
            Self::Alertmanager => "alertmanager",
            Self::Grafana => "grafana",
            Self::Datadog => "datadog",
            Self::Pagerduty => "pagerduty",
            Self::AliyunCms => "aliyun_cms",
            Self::TencentMonitor => "tencent_monitor",
            Self::AwsCloudwatch => "aws_cloudwatch",
            Self::Zabbix => "zabbix",
            Self::Email => "email",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }

    /// 一个事件对应发送方的一个通知分组（Alertmanager、Grafana）。
    pub const fn groups_alerts(self) -> bool {
        matches!(self, Self::Alertmanager | Self::Grafana)
    }
}

/// 一个告警来源。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertSource {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub kind: AlertSourceKind,
    pub enabled: bool,
    /// 它开的事件按哪条升级策略升级（可以是别名 `default`）；`None` 不升级，只按通知策略通知。
    #[serde(default)]
    pub escalation_policy_id: Option<Id>,
    /// 告警自己没带严重度时用的。
    pub severity: Severity,
    /// 加在它开的每个事件上的标签；告警自己带了同名标签时以告警的为准。
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// Token `mspat-<36>` 的前 10 位：按它找来源。
    pub token_prefix: String,
    /// Token 后 26 位的 SHA-256 哈希。随联邦同步，所以同一个 Token 在每个集群都能用；接口不返回它。
    pub token_hash: String,
    /// 在哪个联邦集群开事件（集群 ID）。空表示本集群：没开联邦，或来源建于联邦开启之前。
    #[serde(default)]
    pub home_cluster: String,
    #[serde(default)]
    pub created_by: Option<Id>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
    /// 本集群最近一次收到它的请求。只记在本集群，不随联邦同步。
    #[serde(default)]
    pub last_intake_at: Option<TimestampMicros>,
    /// 邮件来源怎么读一封邮件；别的类型不用。
    #[serde(default)]
    pub email: EmailRules,
}

impl AlertSource {
    /// 名称与附加标签能不能用。
    pub fn validate(&self) -> Result<()> {
        let name = self.name.trim();

        if name.is_empty() {
            return Err(Error::invalid("name cannot be empty"));
        }

        if name.chars().count() > MAX_SOURCE_NAME_CHARS {
            return Err(Error::invalid(format!(
                "name has at most {MAX_SOURCE_NAME_CHARS} characters"
            )));
        }

        if self.labels.len() > MAX_SOURCE_LABELS {
            return Err(Error::invalid(format!(
                "a source has at most {MAX_SOURCE_LABELS} labels"
            )));
        }

        for (key, value) in &self.labels {
            validate_label(key, value)?;
        }

        self.email.validate()
    }

    /// 列表里显示的 Token：`mspat-` 加前 10 位。
    pub fn shown_token(&self) -> String {
        format!("{TOKEN_MARKER}{}", self.token_prefix)
    }

    /// 它的事件在 `local_cluster` 上开：没开联邦（`local_cluster` 为空），来源没定接收集群，或接收
    /// 集群就是本集群。
    pub fn opens_incidents_on(&self, local_cluster: &str) -> bool {
        let home = self.home_cluster.trim();
        local_cluster.trim().is_empty() || home.is_empty() || home == local_cluster.trim()
    }
}

/// 标签键是不是留给 MoleSignal 自己用的：`alert.` 开头、`external.kind`、`__molesignal` 开头。
pub fn is_reserved_label(key: &str) -> bool {
    key.starts_with("alert.") || key == EXTERNAL_KIND_LABEL || key.starts_with("__molesignal")
}

/// 来源上配置的一个附加标签能不能用。
fn validate_label(key: &str, value: &str) -> Result<()> {
    let valid_key = !key.is_empty()
        && key.chars().count() <= MAX_LABEL_KEY_CHARS
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | ':' | '/'));

    if !valid_key {
        return Err(Error::invalid(format!(
            "label key `{key}` must be 1-{MAX_LABEL_KEY_CHARS} letters, digits, `_`, `.`, `-`, `:` or `/`"
        )));
    }

    if is_reserved_label(key) {
        return Err(Error::invalid(format!(
            "label key `{key}` is kept for MoleSignal"
        )));
    }

    if value.chars().count() > MAX_LABEL_VALUE_CHARS {
        return Err(Error::invalid(format!(
            "label `{key}` has at most {MAX_LABEL_VALUE_CHARS} characters"
        )));
    }

    Ok(())
}

#[async_trait]
pub trait AlertSourceRepository: Send + Sync {
    async fn create(&self, source: AlertSource) -> Result<AlertSource>;
    /// 写回定义（含 Token 与接收集群）。`last_intake_at` 由 [`touch_intake`](Self::touch_intake)
    /// 维护，这里不写。
    async fn update(&self, source: AlertSource) -> Result<AlertSource>;
    async fn get(&self, id: &Id) -> Result<AlertSource>;
    async fn list(&self, org_id: &Id) -> Result<Vec<AlertSource>>;
    async fn delete(&self, id: &Id) -> Result<()>;
    /// 按 Token 中间的 16 位找来源。
    async fn find_by_token_prefix(&self, prefix: &str) -> Result<Option<AlertSource>>;
    /// 记下本集群最近一次收到它的请求；只会往后走。
    async fn touch_intake(&self, id: &Id, at: TimestampMicros) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(name: &str, labels: &[(&str, &str)]) -> AlertSource {
        AlertSource {
            id: Id::from_string("src-1"),
            org_id: Id::from_string("org-1"),
            name: name.into(),
            kind: AlertSourceKind::Alertmanager,
            enabled: true,
            escalation_policy_id: None,
            severity: Severity::Warning,
            labels: labels
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
            token_prefix: "c16jb8m5zr".into(),
            token_hash: String::new(),
            home_cluster: String::new(),
            created_by: None,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
            last_intake_at: None,
            email: EmailRules::default(),
        }
    }

    #[test]
    fn kinds_round_trip_through_their_names() {
        for kind in AlertSourceKind::ALL {
            assert_eq!(AlertSourceKind::parse(kind.as_str()), Some(kind));
            assert_eq!(
                serde_json::to_value(kind).unwrap(),
                serde_json::Value::String(kind.as_str().into())
            );
        }

        assert_eq!(
            AlertSourceKind::parse("zabbix"),
            Some(AlertSourceKind::Zabbix)
        );
        assert_eq!(AlertSourceKind::parse("nagios"), None);
        assert!(AlertSourceKind::Grafana.groups_alerts());
        assert!(!AlertSourceKind::Datadog.groups_alerts());
    }

    #[test]
    fn a_source_needs_a_name_and_usable_labels() {
        assert!(
            source("Prometheus · prod", &[("env", "prod")])
                .validate()
                .is_ok()
        );
        assert!(source("  ", &[]).validate().is_err());
        assert!(source("x", &[("alert.source", "x")]).validate().is_err());
        assert!(source("x", &[("external.kind", "x")]).validate().is_err());
        assert!(source("x", &[("has space", "x")]).validate().is_err());
        assert!(source("x", &[("", "x")]).validate().is_err());
    }

    #[test]
    fn the_shown_token_is_its_prefix() {
        assert_eq!(source("x", &[]).shown_token(), "mspat-c16jb8m5zr");
    }

    #[test]
    fn incidents_open_on_the_home_cluster_or_without_federation() {
        let mut source = source("x", &[]);

        assert!(source.opens_incidents_on(""));
        assert!(source.opens_incidents_on("cluster-a"));

        source.home_cluster = "cluster-a".into();
        assert!(source.opens_incidents_on(""));
        assert!(source.opens_incidents_on("cluster-a"));
        assert!(!source.opens_incidents_on("cluster-b"));
    }
}
