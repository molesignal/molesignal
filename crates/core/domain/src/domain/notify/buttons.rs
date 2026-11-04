// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警通知上的按钮：确认、解决、暂缓告警，在 MoleSignal 中查看；几条合成一条的通知上是「全部确认」。
//!
//! 通知引擎按通知的类型与连接器决定画哪些，写进消息的元数据（[`AlertButtons::write`]）；连接器
//! （飞书 App、Slack App）读出来画进卡片。操作按钮带着 [`AlertButtonValue`]，点了由 Mole Agent 的
//! 长连接原样交回来：做什么、对哪个告警，以及按哪种通知、哪个通知策略重写卡片。

use serde::{Deserialize, Serialize};

use super::connector::NotifyMessage;

/// 告警卡片上的一个操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertAction {
    Acknowledge,
    Resolve,
    /// 暂缓一小时（[`SNOOZE_SECS`]）。
    Snooze,
    /// 合成一条的通知上：确认里面全部的告警。
    AcknowledgeAll,
}

/// 卡片上的「暂缓」暂缓多久：1 小时。
pub const SNOOZE_SECS: u32 = 3600;

/// 「全部确认」最多带几个告警。
pub const MAX_BUNDLE_ALERTS: usize = 20;

impl AlertAction {
    pub const ALL: [Self; 4] = [
        Self::Acknowledge,
        Self::Resolve,
        Self::Snooze,
        Self::AcknowledgeAll,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Acknowledge => "acknowledge",
            Self::Resolve => "resolve",
            Self::Snooze => "snooze",
            Self::AcknowledgeAll => "acknowledge_all",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|action| action.as_str() == value)
    }

    /// 告警在这个状态（`open` 之类）时还能做的操作。
    pub fn available(status: &str) -> &'static [Self] {
        match status {
            "open" => &[Self::Acknowledge, Self::Resolve, Self::Snooze],
            "acknowledged" => &[Self::Resolve, Self::Snooze],
            _ => &[],
        }
    }

    /// 元数据里这个按钮上的字。
    const fn label_key(self) -> &'static str {
        match self {
            Self::Acknowledge => "button.acknowledge",
            Self::Resolve => "button.resolve",
            Self::Snooze => "button.snooze",
            Self::AcknowledgeAll => "button.acknowledge_all",
        }
    }
}

const ALERT_ID: &str = "alert_id";
const ALERT_IDS: &str = "alert_ids";
const EVENT_TYPE: &str = "event_type";
const POLICY_ID: &str = "policy_id";
const OPEN_LABEL: &str = "button.open";
const OPEN_URL: &str = "button.open_url";

/// 操作按钮带着的值，点了原样带回来。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertButtonValue {
    pub action: AlertAction,
    pub alert_id: String,
    /// 发出这张卡片的通知（`alert.triggered` 之类）。
    pub event_type: String,
    /// 发出它的通知策略：重写卡片时套用它的模板。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_id: Option<String>,
    /// 「全部确认」确认的告警。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alert_ids: Vec<String>,
}

/// 按钮上的字。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertButtonLabels {
    pub acknowledge: String,
    pub resolve: String,
    pub snooze: String,
    pub acknowledge_all: String,
    /// 「在 MoleSignal 中查看」与 MoleSignal 的对外地址；没有对外地址时不画。
    pub open: Option<(String, String)>,
}

impl AlertButtonLabels {
    pub fn label(&self, action: AlertAction) -> &str {
        match action {
            AlertAction::Acknowledge => &self.acknowledge,
            AlertAction::Resolve => &self.resolve,
            AlertAction::Snooze => &self.snooze,
            AlertAction::AcknowledgeAll => &self.acknowledge_all,
        }
    }
}

/// 一条告警通知上的按钮。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AlertButtons {
    pub alert_id: String,
    pub event_type: String,
    pub policy_id: Option<String>,
    /// 合成一条的通知里的告警：只有「全部确认」用它。
    pub alert_ids: Vec<String>,
    /// 操作按钮与按钮上的字，按 [`AlertAction::ALL`] 的顺序。
    pub actions: Vec<(AlertAction, String)>,
    /// 「在 MoleSignal 中查看」：按钮上的字与地址。
    pub open: Option<(String, String)>,
}

impl AlertButtons {
    /// `alert_id` 这个告警在 `status` 时的按钮，写着 `labels` 的字。`event_type` 与 `policy_id`
    /// 是发出这张卡片的通知与通知策略。
    pub fn new(
        alert_id: &str,
        status: &str,
        event_type: &str,
        policy_id: Option<&str>,
        labels: &AlertButtonLabels,
    ) -> Self {
        Self {
            alert_id: alert_id.to_string(),
            event_type: event_type.to_string(),
            policy_id: policy_id.map(str::to_string),
            alert_ids: Vec::new(),
            actions: AlertAction::available(status)
                .iter()
                .map(|action| (*action, labels.label(*action).to_string()))
                .collect(),
            open: labels.open.as_ref().map(|(label, base)| {
                (
                    label.clone(),
                    format!(
                        "{}/alerts/incidents/{alert_id}",
                        base.trim().trim_end_matches('/')
                    ),
                )
            }),
        }
    }

    /// 合成一条的通知上的按钮：「全部确认」它里面的告警（最多 [`MAX_BUNDLE_ALERTS`] 个），「在
    /// MoleSignal 中查看」打开事件列表。
    pub fn bundle(
        alert_ids: &[String],
        event_type: &str,
        policy_id: Option<&str>,
        labels: &AlertButtonLabels,
    ) -> Self {
        let alert_ids: Vec<String> = alert_ids.iter().take(MAX_BUNDLE_ALERTS).cloned().collect();

        Self {
            alert_id: alert_ids.first().cloned().unwrap_or_default(),
            event_type: event_type.to_string(),
            policy_id: policy_id.map(str::to_string),
            actions: vec![(AlertAction::AcknowledgeAll, labels.acknowledge_all.clone())],
            open: labels.open.as_ref().map(|(label, base)| {
                (
                    label.clone(),
                    format!("{}/alerts/incidents", base.trim().trim_end_matches('/')),
                )
            }),
            alert_ids,
        }
    }

    /// 一个操作按钮带着的值。
    pub fn value(&self, action: AlertAction) -> AlertButtonValue {
        AlertButtonValue {
            action,
            alert_id: self.alert_id.clone(),
            event_type: self.event_type.clone(),
            policy_id: self.policy_id.clone(),
            alert_ids: if action == AlertAction::AcknowledgeAll {
                self.alert_ids.clone()
            } else {
                Vec::new()
            },
        }
    }

    /// 写进消息的元数据。
    pub fn write(&self, message: &mut NotifyMessage) {
        let metadata = &mut message.metadata;

        metadata.insert(ALERT_ID.into(), self.alert_id.clone());
        metadata.insert(EVENT_TYPE.into(), self.event_type.clone());

        if let Some(policy_id) = &self.policy_id {
            metadata.insert(POLICY_ID.into(), policy_id.clone());
        }

        if !self.alert_ids.is_empty() {
            metadata.insert(ALERT_IDS.into(), self.alert_ids.join(","));
        }

        for (action, label) in &self.actions {
            metadata.insert(action.label_key().into(), label.clone());
        }

        if let Some((label, url)) = &self.open {
            metadata.insert(OPEN_LABEL.into(), label.clone());
            metadata.insert(OPEN_URL.into(), url.clone());
        }
    }

    /// 消息带着的按钮；不是告警通知或一个操作按钮也没有时为 `None`。
    pub fn read(message: &NotifyMessage) -> Option<Self> {
        let metadata = &message.metadata;

        let actions = AlertAction::ALL
            .into_iter()
            .filter_map(|action| {
                metadata
                    .get(action.label_key())
                    .map(|label| (action, label.clone()))
            })
            .collect::<Vec<_>>();

        if actions.is_empty() {
            return None;
        }

        Some(Self {
            alert_id: metadata.get(ALERT_ID)?.clone(),
            event_type: metadata.get(EVENT_TYPE).cloned().unwrap_or_default(),
            policy_id: metadata.get(POLICY_ID).cloned(),
            alert_ids: metadata
                .get(ALERT_IDS)
                .map(|ids| {
                    ids.split(',')
                        .filter(|id| !id.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            actions,
            open: metadata
                .get(OPEN_LABEL)
                .zip(metadata.get(OPEN_URL))
                .map(|(label, url)| (label.clone(), url.clone())),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn message() -> NotifyMessage {
        NotifyMessage {
            title: "CRITICAL · API unavailable".into(),
            text: "Alert API unavailable is open.".into(),
            markdown: None,
            html: None,
            metadata: BTreeMap::from([
                ("alert_id".to_string(), "incident-1".to_string()),
                ("event_type".to_string(), "alert.triggered".to_string()),
            ]),
            provider_templates: Default::default(),
            event: None,
            card: None,
        }
    }

    fn labels() -> AlertButtonLabels {
        AlertButtonLabels {
            acknowledge: "确认".into(),
            resolve: "解决".into(),
            snooze: "暂缓 1 小时".into(),
            acknowledge_all: "全部确认".into(),
            open: Some((
                "在 MoleSignal 中查看".into(),
                "https://molesignal.example/".into(),
            )),
        }
    }

    #[test]
    fn an_alert_offers_what_its_status_still_allows() {
        let open = AlertButtons::new(
            "incident-1",
            "open",
            "alert.triggered",
            Some("policy-1"),
            &labels(),
        );

        assert_eq!(
            open.actions,
            vec![
                (AlertAction::Acknowledge, "确认".to_string()),
                (AlertAction::Resolve, "解决".to_string()),
                (AlertAction::Snooze, "暂缓 1 小时".to_string()),
            ]
        );
        assert_eq!(
            open.open,
            Some((
                "在 MoleSignal 中查看".to_string(),
                "https://molesignal.example/alerts/incidents/incident-1".to_string(),
            ))
        );

        let acknowledged = AlertButtons::new(
            "incident-1",
            "acknowledged",
            "alert.triggered",
            None,
            &labels(),
        );
        assert_eq!(
            acknowledged.actions,
            vec![
                (AlertAction::Resolve, "解决".to_string()),
                (AlertAction::Snooze, "暂缓 1 小时".to_string()),
            ]
        );

        let resolved =
            AlertButtons::new("incident-1", "resolved", "alert.triggered", None, &labels());
        assert!(resolved.actions.is_empty());
        assert!(resolved.open.is_some());
    }

    #[test]
    fn buttons_go_through_the_metadata_of_a_message() {
        let buttons = AlertButtons::new(
            "incident-1",
            "open",
            "alert.triggered",
            Some("policy-1"),
            &labels(),
        );

        let mut message = message();
        assert_eq!(AlertButtons::read(&message), None);

        buttons.write(&mut message);
        assert_eq!(AlertButtons::read(&message), Some(buttons.clone()));

        let value = buttons.value(AlertAction::Resolve);
        let written = serde_json::to_value(&value).unwrap();
        assert_eq!(
            written,
            serde_json::json!({
                "action": "resolve",
                "alert_id": "incident-1",
                "event_type": "alert.triggered",
                "policy_id": "policy-1",
            })
        );
        assert_eq!(
            serde_json::from_value::<AlertButtonValue>(written).unwrap(),
            value
        );
    }

    #[test]
    fn a_message_with_only_a_link_draws_no_card() {
        let mut message = message();

        AlertButtons::new("incident-1", "resolved", "alert.resolved", None, &labels())
            .write(&mut message);

        assert_eq!(AlertButtons::read(&message), None);
        assert_eq!(
            AlertAction::parse("acknowledge"),
            Some(AlertAction::Acknowledge)
        );
        assert_eq!(AlertAction::parse("close"), None);
    }

    #[test]
    fn a_merged_notification_acknowledges_all_its_alerts_at_once() {
        let ids: Vec<String> = (0..30).map(|index| format!("inc-{index}")).collect();
        let buttons = AlertButtons::bundle(&ids, "alert.triggered", Some("policy-1"), &labels());

        assert_eq!(buttons.alert_ids.len(), MAX_BUNDLE_ALERTS);
        assert_eq!(
            buttons.actions,
            vec![(AlertAction::AcknowledgeAll, "全部确认".to_string())]
        );
        assert_eq!(
            buttons.open.as_ref().map(|(_, url)| url.as_str()),
            Some("https://molesignal.example/alerts/incidents")
        );

        let mut message = message();
        buttons.write(&mut message);
        let read = AlertButtons::read(&message).expect("bundle buttons");
        assert_eq!(read.alert_ids, buttons.alert_ids);

        let value = read.value(AlertAction::AcknowledgeAll);
        assert_eq!(value.alert_ids.len(), MAX_BUNDLE_ALERTS);
        assert!(read.value(AlertAction::Acknowledge).alert_ids.is_empty());
    }
}
