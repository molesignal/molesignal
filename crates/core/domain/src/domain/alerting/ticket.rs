// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 工单：把事件同步到 Jira、ServiceNow 或飞书项目（Meegle）。
//!
//! - 连接器存站点、凭证（加密）、建单用的项目与类型、严重度到优先级的映射，以及解决事件时怎么
//!   流转、哪些状态算完结。
//! - 什么时候建单：事件上手动建；自动规则（标签条件同路由规则，加最低严重度）；宣布重大事件时；
//!   复盘改进项一项一单。
//! - 回传：每 5 分钟轮询关联中的工单，Jira 也可以配 Webhook。工单完结时默认不解决事件，连接器上可开。
//!   哪些状态算完结：Jira 看状态类别，飞书项目和 ServiceNow 看连接器上列的状态。

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    incident::{Incident, Severity},
    routing::{LabelCondition, conditions_hold, validate_label_conditions},
};
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

/// 关联中的工单多久同步一次。
pub const TICKET_SYNC_INTERVAL_MICROS: i64 = 5 * 60 * 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketSystem {
    Jira,
    Meegle,
    Servicenow,
}

impl TicketSystem {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Jira => "jira",
            Self::Meegle => "meegle",
            Self::Servicenow => "servicenow",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "jira" => Ok(Self::Jira),
            "meegle" => Ok(Self::Meegle),
            "servicenow" => Ok(Self::Servicenow),
            other => Err(Error::invalid(format!("unknown ticket system `{other}`"))),
        }
    }
}

/// 自动建单：事件标签满足条件、严重度不低于下限时建一张单。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketAutoRule {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub conditions: Vec<LabelCondition>,
    #[serde(default)]
    pub min_severity: Option<Severity>,
}

impl TicketAutoRule {
    pub fn matches(&self, incident: &Incident) -> bool {
        self.enabled
            && self
                .min_severity
                .is_none_or(|floor| incident.severity >= floor)
            && conditions_hold(&self.conditions, &incident.labels)
    }
}

/// 事件上的变化写不写成工单评论。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketComments {
    #[serde(default)]
    pub acknowledged: bool,
    #[serde(default)]
    pub resolved: bool,
    #[serde(default)]
    pub notes: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketConnector {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub system: TicketSystem,
    /// 不是秘密的设置：站点、项目、类型、映射、流转（见 `JiraSettings`、`MeegleSettings`）。
    pub settings: Value,
    /// 秘密：Jira 的 API Token / PAT 与 Webhook 密钥，飞书项目的插件密钥，ServiceNow 的密码、客户端密钥或
    /// API Key。读出来给页面时不带。
    #[serde(default, skip_serializing)]
    pub secrets: BTreeMap<String, String>,
    /// 页面上看哪些秘密已经设了。
    #[serde(default)]
    pub secrets_set: Vec<String>,
    #[serde(default)]
    pub comments: TicketComments,
    /// 解决事件时把工单流转到完结。
    #[serde(default)]
    pub transition_on_resolve: bool,
    /// 工单完结时解决事件。
    #[serde(default)]
    pub resolve_incident_when_done: bool,
    #[serde(default)]
    pub auto: TicketAutoRule,
    /// 宣布重大事件时建单。
    #[serde(default)]
    pub on_major_incident: bool,
    pub enabled: bool,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// Jira 部署方式：Cloud 用邮箱 + API Token 和 REST v3；Data Center 用个人访问令牌和 REST v2。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JiraDeployment {
    #[default]
    Cloud,
    DataCenter,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraSettings {
    pub site_url: String,
    #[serde(default)]
    pub deployment: JiraDeployment,
    /// Cloud 的账号邮箱。
    #[serde(default)]
    pub email: String,
    pub project_key: String,
    pub issue_type: String,
    /// 严重度 → 优先级名字；没写的不设优先级。
    #[serde(default)]
    pub priorities: BTreeMap<String, String>,
    #[serde(default)]
    pub labels: Vec<String>,
    /// 建单时其他字段的默认值（Jira 字段 ID → 值）。
    #[serde(default)]
    pub fields: serde_json::Map<String, Value>,
    /// 解决事件时走的流转（名字或 ID）；空着时找第一个通往完结类状态的流转。
    #[serde(default)]
    pub resolve_transition: String,
}

/// 飞书项目的平台：国内是飞书项目，海外是 Meegle。地址随平台定，不能改。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeegleSite {
    #[default]
    Feishu,
    Lark,
}

/// 各平台的地址，第一个是默认的。
pub const MEEGLE_SITES: [(MeegleSite, &str); 2] = [
    (MeegleSite::Feishu, "https://project.feishu.cn"),
    (MeegleSite::Lark, "https://meegle.com"),
];

impl MeegleSite {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Feishu => "feishu",
            Self::Lark => "lark",
        }
    }

    pub fn base_url(self) -> &'static str {
        MEEGLE_SITES
            .iter()
            .find(|(site, _)| *site == self)
            .map_or(MEEGLE_SITES[0].1, |(_, url)| url)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeegleSettings {
    /// 平台：飞书项目或 Meegle，地址随它定。
    #[serde(default)]
    pub site: MeegleSite,
    pub plugin_id: String,
    /// 以谁的身份（user_key）。
    pub user_key: String,
    /// 空间（project_key）。
    pub project_key: String,
    pub work_item_type_key: String,
    /// 优先级字段的 key；空着不设优先级。
    #[serde(default)]
    pub priority_field: String,
    /// 严重度 → 优先级选项的值。
    #[serde(default)]
    pub priorities: BTreeMap<String, String>,
    /// 建单时其他字段的默认值（field_key → 值）。
    #[serde(default)]
    pub fields: serde_json::Map<String, Value>,
    /// 状态流：解决事件时走的流转 ID。
    #[serde(default)]
    pub resolve_transition_id: Option<i64>,
    /// 节点流：解决事件时完成的节点 ID。
    #[serde(default)]
    pub resolve_node_id: String,
    /// 算完结的状态 key。
    #[serde(default)]
    pub done_states: Vec<String>,
}

/// ServiceNow 怎么认证。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceNowAuth {
    /// 用户名和密码（秘密 `password`）。
    #[default]
    Basic,
    /// OAuth 客户端凭证（秘密 `client_secret`）。
    Oauth,
    /// API Key（秘密 `api_key`）。
    ApiKey,
}

fn servicenow_table() -> String {
    "incident".into()
}

fn servicenow_resolve_state() -> String {
    "6".into()
}

fn servicenow_close_code() -> String {
    "Solution provided".into()
}

fn servicenow_done_states() -> Vec<String> {
    ["6", "7", "8"].map(str::to_string).to_vec()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceNowSettings {
    /// 实例地址，如 `https://acme.service-now.com`。
    pub instance_url: String,
    #[serde(default)]
    pub auth: ServiceNowAuth,
    /// 基本认证的用户名。
    #[serde(default)]
    pub username: String,
    /// OAuth 的客户端 ID。
    #[serde(default)]
    pub client_id: String,
    /// 建单的表；要继承 `task`。
    #[serde(default = "servicenow_table")]
    pub table: String,
    /// 分派组（`sys_id`）。
    #[serde(default)]
    pub assignment_group: String,
    /// 提单人（`sys_id`）。
    #[serde(default)]
    pub caller_id: String,
    #[serde(default)]
    pub category: String,
    /// 严重度或优先级 → `impact/urgency`，只写一个数字时两个都用它；没写的不设。
    #[serde(default)]
    pub priorities: BTreeMap<String, String>,
    /// 建单时其他字段的默认值（字段名 → 值）。
    #[serde(default)]
    pub fields: serde_json::Map<String, Value>,
    /// 解决事件时写的 `state` 的值。
    #[serde(default = "servicenow_resolve_state")]
    pub resolve_state: String,
    /// 解决事件时写的 `close_code`；空着不写。各实例的选项不一样。
    #[serde(default = "servicenow_close_code")]
    pub close_code: String,
    /// 算完结的 `state` 的值。
    #[serde(default = "servicenow_done_states")]
    pub done_states: Vec<String>,
}

impl TicketConnector {
    pub fn servicenow(&self) -> Result<ServiceNowSettings> {
        serde_json::from_value(self.settings.clone())
            .map_err(|error| Error::invalid(format!("ServiceNow settings: {error}")))
    }

    pub fn jira(&self) -> Result<JiraSettings> {
        serde_json::from_value(self.settings.clone())
            .map_err(|error| Error::invalid(format!("Jira settings: {error}")))
    }

    pub fn meegle(&self) -> Result<MeegleSettings> {
        serde_json::from_value(self.settings.clone())
            .map_err(|error| Error::invalid(format!("Meegle settings: {error}")))
    }

    pub fn secret(&self, key: &str) -> Option<&str> {
        self.secrets
            .get(key)
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
    }

    /// 名字、站点与系统要的字段都在。
    pub fn validate(&mut self) -> Result<()> {
        self.name = self.name.trim().to_string();

        if self.name.is_empty() || self.name.chars().count() > 120 {
            return Err(Error::invalid("a connector's name has 1 to 120 characters"));
        }

        validate_label_conditions(&self.auto.conditions)?;

        match self.system {
            TicketSystem::Jira => {
                let mut jira = self.jira()?;
                jira.site_url = site(&jira.site_url)?;
                jira.project_key = jira.project_key.trim().to_string();
                jira.issue_type = jira.issue_type.trim().to_string();

                if jira.project_key.is_empty() || jira.issue_type.is_empty() {
                    return Err(Error::invalid(
                        "Jira needs the project key and the issue type",
                    ));
                }

                if jira.deployment == JiraDeployment::Cloud && jira.email.trim().is_empty() {
                    return Err(Error::invalid(
                        "Jira Cloud signs in with the account's email",
                    ));
                }

                self.settings = serde_json::to_value(jira)
                    .map_err(|error| Error::internal(format!("Jira settings: {error}")))?;
            }
            TicketSystem::Meegle => {
                let meegle = self.meegle()?;

                let named = [
                    &meegle.plugin_id,
                    &meegle.user_key,
                    &meegle.project_key,
                    &meegle.work_item_type_key,
                ];

                if named.iter().any(|value| value.trim().is_empty()) {
                    return Err(Error::invalid(
                        "Meegle needs the plugin ID, user key, project key and work item type",
                    ));
                }

                self.settings = serde_json::to_value(meegle)
                    .map_err(|error| Error::internal(format!("Meegle settings: {error}")))?;
            }
            TicketSystem::Servicenow => {
                let mut servicenow = self.servicenow()?;
                servicenow.validate()?;

                self.settings = serde_json::to_value(servicenow)
                    .map_err(|error| Error::internal(format!("ServiceNow settings: {error}")))?;
            }
        }

        Ok(())
    }
}

/// 优先级映射里写的 `impact/urgency`，或一个数字两边都用。ServiceNow 的这两个字段是数字。
pub fn servicenow_impact_and_urgency(mapped: &str) -> Option<(String, String)> {
    let mapped = mapped.trim();

    let (impact, urgency) = match mapped.split_once('/') {
        Some((impact, urgency)) => (impact.trim(), urgency.trim()),
        None => (mapped, mapped),
    };

    let number = |value: &str| !value.is_empty() && value.chars().all(|c| c.is_ascii_digit());

    (number(impact) && number(urgency)).then(|| (impact.to_string(), urgency.to_string()))
}

impl ServiceNowSettings {
    fn validate(&mut self) -> Result<()> {
        self.instance_url = site(&self.instance_url)?;
        self.table = self.table.trim().to_string();
        self.resolve_state = self.resolve_state.trim().to_string();

        let table_name = !self.table.is_empty()
            && self
                .table
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');

        if !table_name {
            return Err(Error::invalid(
                "the ServiceNow table is a name like `incident`",
            ));
        }

        match self.auth {
            ServiceNowAuth::Basic if self.username.trim().is_empty() => {
                return Err(Error::invalid("basic authentication needs the user name"));
            }
            ServiceNowAuth::Oauth if self.client_id.trim().is_empty() => {
                return Err(Error::invalid("OAuth needs the client ID"));
            }
            _ => {}
        }

        self.priorities
            .retain(|_, mapped| !mapped.trim().is_empty());

        // 写错了不会报错，只是建单时没有优先级：保存时就说。
        if let Some((key, mapped)) = self
            .priorities
            .iter()
            .find(|(_, mapped)| servicenow_impact_and_urgency(mapped).is_none())
        {
            return Err(Error::invalid(format!(
                "the ServiceNow priority of `{key}` is impact/urgency like 2/1, or one number: \
                 `{mapped}` is neither"
            )));
        }

        if self.resolve_state.is_empty() {
            self.resolve_state = servicenow_resolve_state();
        }

        self.done_states = std::mem::take(&mut self.done_states)
            .into_iter()
            .map(|state| state.trim().to_string())
            .filter(|state| !state.is_empty())
            .collect();

        if self.done_states.is_empty() {
            self.done_states = servicenow_done_states();
        }

        Ok(())
    }

    /// 这种认证方式要的秘密。
    pub const fn required_secret(&self) -> &'static str {
        match self.auth {
            ServiceNowAuth::Basic => "password",
            ServiceNowAuth::Oauth => "clientSecret",
            ServiceNowAuth::ApiKey => "apiKey",
        }
    }
}

/// 站点地址：https 开头，去掉结尾的斜杠。
fn site(url: &str) -> Result<String> {
    let url = url.trim().trim_end_matches('/');

    if !url.to_ascii_lowercase().starts_with("https://") {
        return Err(Error::invalid("the site address starts with https://"));
    }

    Ok(url.to_string())
}

/// 一个事件关联的一张工单。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncidentTicket {
    pub id: Id,
    pub org_id: Id,
    pub incident_id: Id,
    pub connector_id: Id,
    pub system: TicketSystem,
    /// Jira 的 issue ID、飞书项目的工作项 ID、ServiceNow 的 `sys_id`。
    pub external_id: String,
    /// Jira 的 issue key（如 `OPS-42`）、ServiceNow 的编号（如 `INC0010042`）；飞书项目同 ID。
    pub external_key: String,
    pub url: String,
    /// 工单现在的状态名。
    #[serde(default)]
    pub status: String,
    pub done: bool,
    /// 上次同步时事件的状态，看出事件从那以后有没有被确认、解决。
    #[serde(default)]
    pub incident_status: String,
    pub created_by: Option<Id>,
    pub created_at: TimestampMicros,
    pub synced_at: TimestampMicros,
    #[serde(default)]
    pub last_error: String,
    /// 为复盘改进项建的单：不跟着事件写评论、流转。
    #[serde(default)]
    pub action_item_id: Option<Id>,
}

#[async_trait]
pub trait TicketRepository: Send + Sync {
    async fn list_connectors(&self, org_id: &Id) -> Result<Vec<TicketConnector>>;
    /// 带着秘密读出。
    async fn get_connector(&self, org_id: &Id, id: &Id) -> Result<Option<TicketConnector>>;
    /// Jira Webhook 进来时按地址里的连接器 ID 找，带着秘密。
    async fn find_connector(&self, id: &Id) -> Result<Option<TicketConnector>>;
    async fn save_connector(&self, connector: TicketConnector) -> Result<TicketConnector>;
    async fn delete_connector(&self, org_id: &Id, id: &Id) -> Result<bool>;

    async fn create_ticket(&self, ticket: IncidentTicket) -> Result<IncidentTicket>;
    async fn update_ticket(&self, ticket: IncidentTicket) -> Result<IncidentTicket>;
    async fn list_for_incident(&self, org_id: &Id, incident_id: &Id)
    -> Result<Vec<IncidentTicket>>;
    /// 没完结、上次同步早于 `before` 的。
    async fn due_for_sync(
        &self,
        before: TimestampMicros,
        limit: i64,
    ) -> Result<Vec<IncidentTicket>>;
    async fn find_by_external(
        &self,
        connector_id: &Id,
        external_key: &str,
    ) -> Result<Option<IncidentTicket>>;
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::alerting::routing::ConditionOperator;

    fn connector(system: TicketSystem, settings: Value) -> TicketConnector {
        TicketConnector {
            id: Id::from_string("tc-1"),
            org_id: Id::from_string("org-1"),
            name: " Ops Jira ".into(),
            system,
            settings,
            secrets: BTreeMap::new(),
            secrets_set: Vec::new(),
            comments: TicketComments::default(),
            transition_on_resolve: false,
            resolve_incident_when_done: false,
            auto: TicketAutoRule::default(),
            on_major_incident: false,
            enabled: true,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    #[test]
    fn jira_needs_its_project_and_a_secure_site() {
        let mut fine = connector(
            TicketSystem::Jira,
            json!({"siteUrl": "https://acme.atlassian.net/", "email": "ops@acme.com", "projectKey": "OPS", "issueType": "Incident"}),
        );
        assert!(fine.validate().is_ok());
        assert_eq!(fine.name, "Ops Jira");
        assert_eq!(fine.jira().unwrap().site_url, "https://acme.atlassian.net");

        let mut http = connector(
            TicketSystem::Jira,
            json!({"siteUrl": "http://jira.local", "email": "ops@acme.com", "projectKey": "OPS", "issueType": "Bug"}),
        );
        assert!(http.validate().is_err());

        let mut no_email = connector(
            TicketSystem::Jira,
            json!({"siteUrl": "https://acme.atlassian.net", "projectKey": "OPS", "issueType": "Bug"}),
        );
        assert!(no_email.validate().is_err());
    }

    #[test]
    fn meegle_names_its_plugin_space_and_type() {
        let mut fine = connector(
            TicketSystem::Meegle,
            json!({"site": "feishu", "pluginId": "MII_1", "userKey": "u1", "projectKey": "ops", "workItemTypeKey": "issue"}),
        );
        assert!(fine.validate().is_ok());

        let mut missing = connector(
            TicketSystem::Meegle,
            json!({"site": "feishu", "pluginId": "MII_1", "userKey": "", "projectKey": "ops", "workItemTypeKey": "issue"}),
        );
        assert!(missing.validate().is_err());
    }

    #[test]
    fn servicenow_priorities_are_impact_and_urgency() {
        let with = |priorities: Value| {
            let mut fine = connector(
                TicketSystem::Servicenow,
                json!({"instanceUrl": "https://acme.service-now.com", "username": "svc", "priorities": priorities}),
            );

            fine.secrets.insert("password".into(), "p".into());
            fine
        };

        let mut fine = with(json!({"critical": "1/1", "error": " 2 / 1 ", "P1": "3", "info": " "}));

        assert!(fine.validate().is_ok());
        assert!(!fine.servicenow().unwrap().priorities.contains_key("info"));

        for wrong in ["urgent", "1/", "/2", "1/high", "1/2/3"] {
            assert!(
                with(json!({"critical": wrong})).validate().is_err(),
                "{wrong}"
            );
        }

        assert_eq!(
            servicenow_impact_and_urgency(" 2 / 1 "),
            Some(("2".into(), "1".into()))
        );
        assert_eq!(
            servicenow_impact_and_urgency(" 3 "),
            Some(("3".into(), "3".into()))
        );
        assert_eq!(servicenow_impact_and_urgency(""), None);
    }

    #[test]
    fn servicenow_needs_a_secure_instance_and_what_its_authentication_uses() {
        let mut fine = connector(
            TicketSystem::Servicenow,
            json!({"instanceUrl": "https://acme.service-now.com/", "username": "molesignal"}),
        );
        assert!(fine.validate().is_ok());

        let settings = fine.servicenow().unwrap();
        assert_eq!(settings.instance_url, "https://acme.service-now.com");
        assert_eq!(settings.table, "incident");
        assert_eq!(settings.done_states, vec!["6", "7", "8"]);
        assert_eq!(settings.resolve_state, "6");
        assert_eq!(settings.required_secret(), "password");

        let mut plain = connector(
            TicketSystem::Servicenow,
            json!({"instanceUrl": "http://acme.local", "username": "u"}),
        );
        assert!(plain.validate().is_err());

        let mut no_user = connector(
            TicketSystem::Servicenow,
            json!({"instanceUrl": "https://acme.service-now.com"}),
        );
        assert!(no_user.validate().is_err());

        let mut no_client = connector(
            TicketSystem::Servicenow,
            json!({"instanceUrl": "https://acme.service-now.com", "auth": "oauth"}),
        );
        assert!(no_client.validate().is_err());

        let mut bad_table = connector(
            TicketSystem::Servicenow,
            json!({"instanceUrl": "https://acme.service-now.com", "username": "u", "table": "incident?x=1"}),
        );
        assert!(bad_table.validate().is_err());

        let mut empty_done = connector(
            TicketSystem::Servicenow,
            json!({"instanceUrl": "https://acme.service-now.com", "auth": "api_key", "doneStates": [" "]}),
        );
        assert!(empty_done.validate().is_ok());
        assert_eq!(empty_done.servicenow().unwrap().done_states.len(), 3);
        assert_eq!(empty_done.servicenow().unwrap().required_secret(), "apiKey");
    }

    #[test]
    fn an_auto_rule_needs_its_labels_and_severity() {
        let rule = TicketAutoRule {
            enabled: true,
            conditions: vec![LabelCondition {
                key: "service".into(),
                operator: ConditionOperator::Equals,
                values: vec!["checkout".into()],
            }],
            min_severity: Some(Severity::Error),
        };

        let mut labels = BTreeMap::new();
        labels.insert("service".to_string(), "checkout".to_string());

        let incident = |severity: Severity, labels: BTreeMap<String, String>| Incident {
            id: Id::from_string("inc-1"),
            org_id: Id::from_string("org-1"),
            rule_id: Id::from_string("rule-1"),
            escalation_policy_id: Id::from_string("default"),
            status: crate::domain::alerting::incident::IncidentStatus::Open,
            severity,
            summary: "Checkout errors".into(),
            fingerprint: "fp".into(),
            current_step: 0,
            current_loop: 0,
            current_step_started_at: TimestampMicros(0),
            assignees: Vec::new(),
            labels,
            annotations: BTreeMap::new(),
            trace_ids: Vec::new(),
            host_ids: Vec::new(),
            affected_services: Vec::new(),
            triggering_query: None,
            created_at: TimestampMicros(0),
            acknowledged_at: None,
            acknowledged_by: None,
            resolved_at: None,
            resolved_by: None,
            team_id: None,
            routing: None,
            handling: Default::default(),
            revision: 0,
        };

        assert!(rule.matches(&incident(Severity::Critical, labels.clone())));
        assert!(!rule.matches(&incident(Severity::Warning, labels.clone())));
        assert!(!rule.matches(&incident(Severity::Critical, BTreeMap::new())));
        assert!(
            !TicketAutoRule {
                enabled: false,
                ..rule
            }
            .matches(&incident(Severity::Critical, labels))
        );
    }
}
