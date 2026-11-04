// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod blocks;
mod buttons;
mod calls;
mod context;
mod escalation;
mod events;
mod execution;
mod follow_ups;
mod inhibitions;
mod management;
mod model;
mod pipelines;
mod reachability;
mod replies;
mod routes;
mod silences;
mod template;
#[cfg(test)]
pub(super) mod tests;
mod webhook;

use std::sync::Arc;

pub use model::{
    NotifyDefaultInput, NotifyDeliveryPlanStep, NotifyDispatch, NotifyEventOutcome,
    NotifyPolicyInput, NotifyPolicyOutcome, NotifyPolicyPreview, NotifyRecipientOutcome,
    NotifyRecipientPlan,
};
pub use reachability::Reachability;
pub use replies::reply_line_key;
pub(super) use template::render_sample;
pub use template::validate_notify_template;

use super::{AlertButtonSlot, AlertButtonSource, ConnectorRegistry, RecipientResolverRegistry};
use crate::domain::{
    alerting::{
        mute::MuteRuleRepository,
        repositories::{AlertRuleRepository, IncidentRepository},
    },
    iam::TeamRepository,
    notify::repositories::{
        NotifyCallRepository, NotifyConnectorRepository, NotifyDeliveryRepository,
        NotifyEventRepository, NotifyFollowUpRepository, NotifyPolicyRepository,
        NotifyTemplateRepository, OrganizationNotifyDefaultRepository, TeamNotifyDefaultRepository,
        UserNotifyEndpointRepository, UserNotifyPreferenceRepository,
    },
};

pub struct NotifyEngineDependencies {
    pub connectors: Arc<dyn NotifyConnectorRepository>,
    pub endpoints: Arc<dyn UserNotifyEndpointRepository>,
    pub preferences: Arc<dyn UserNotifyPreferenceRepository>,
    pub deliveries: Arc<dyn NotifyDeliveryRepository>,
    pub events: Arc<dyn NotifyEventRepository>,
    pub follow_ups: Arc<dyn NotifyFollowUpRepository>,
    pub calls: Arc<dyn NotifyCallRepository>,
    pub policies: Arc<dyn NotifyPolicyRepository>,
    pub team_defaults: Arc<dyn TeamNotifyDefaultRepository>,
    pub organization_defaults: Arc<dyn OrganizationNotifyDefaultRepository>,
    pub teams: Arc<dyn TeamRepository>,
    pub templates: Arc<dyn NotifyTemplateRepository>,
    pub connector_registry: Arc<ConnectorRegistry>,
    pub resolver_registry: Arc<RecipientResolverRegistry>,
}

pub struct NotifyEngine {
    connectors: Arc<dyn NotifyConnectorRepository>,
    endpoints: Arc<dyn UserNotifyEndpointRepository>,
    preferences: Arc<dyn UserNotifyPreferenceRepository>,
    deliveries: Arc<dyn NotifyDeliveryRepository>,
    events: Arc<dyn NotifyEventRepository>,
    follow_ups: Arc<dyn NotifyFollowUpRepository>,
    calls: Arc<dyn NotifyCallRepository>,
    policies: Arc<dyn NotifyPolicyRepository>,
    team_defaults: Arc<dyn TeamNotifyDefaultRepository>,
    organization_defaults: Arc<dyn OrganizationNotifyDefaultRepository>,
    teams: Arc<dyn TeamRepository>,
    templates: Arc<dyn NotifyTemplateRepository>,
    connector_registry: Arc<ConnectorRegistry>,
    resolver_registry: Arc<RecipientResolverRegistry>,
    alert_buttons: AlertButtonSlot,
    /// 告警事件从这里补规则的说明与阈值；没有时不补。
    alert_rules: Option<Arc<dyn AlertRuleRepository>>,
    /// MoleSignal 的对外地址（`http.external_url`），告警的地址在它下面；空着时不给告警地址。
    external_url: String,
    /// 告警通知看哪里的静默；没有时不看静默。
    silences: Option<silences::Silences>,
    /// 回复短信、电话按键的引用记在哪里；没有时不收回复。
    replies: Option<Arc<dyn crate::domain::notify::reply::NotifyReplyRepository>>,
    /// 组织的优先级与紧急度档位：告警通知按它取个人的步骤、判断免打扰；没有时按内置的高、低。
    importance: Option<Arc<crate::app::alerting::ImportanceService>>,
    /// 策略发送告警通知之前经过的告警流水线；没有时照原样发。
    alert_pipelines: Option<Arc<crate::app::alerting::event_pipelines::AlertPipelines>>,
}

impl NotifyEngine {
    pub fn new(dependencies: NotifyEngineDependencies) -> Self {
        Self {
            connectors: dependencies.connectors,
            endpoints: dependencies.endpoints,
            preferences: dependencies.preferences,
            deliveries: dependencies.deliveries,
            events: dependencies.events,
            follow_ups: dependencies.follow_ups,
            calls: dependencies.calls,
            policies: dependencies.policies,
            team_defaults: dependencies.team_defaults,
            organization_defaults: dependencies.organization_defaults,
            teams: dependencies.teams,
            templates: dependencies.templates,
            connector_registry: dependencies.connector_registry,
            resolver_registry: dependencies.resolver_registry,
            alert_buttons: AlertButtonSlot::default(),
            alert_rules: None,
            external_url: String::new(),
            silences: None,
            replies: None,
            importance: None,
            alert_pipelines: None,
        }
    }

    /// 告警通知按组织的紧急度档位取步骤、判断免打扰，模板能用档的名称。
    pub fn with_importance(
        mut self,
        importance: Arc<crate::app::alerting::ImportanceService>,
    ) -> Self {
        self.importance = Some(importance);
        self
    }

    /// 告警事件从哪里补规则的说明与阈值，告警的地址在哪个站点下。
    pub fn with_alert_context(
        mut self,
        alert_rules: Arc<dyn AlertRuleRepository>,
        external_url: String,
    ) -> Self {
        self.alert_rules = Some(alert_rules);
        self.external_url = external_url;
        self
    }

    /// 告警通知按哪里的静默暂缓，静默结束时去哪里看告警事件还开着没有。
    pub fn with_silences(
        mut self,
        mute_rules: Arc<dyn MuteRuleRepository>,
        incidents: Arc<dyn IncidentRepository>,
    ) -> Self {
        self.silences = Some(silences::Silences::new(mute_rules, incidents));
        self
    }

    /// 告警通知上的按钮从哪里来（Mole Agent 建好以后）。
    pub fn fill_alert_buttons(&self, source: Arc<dyn AlertButtonSource>) {
        self.alert_buttons.fill(source);
    }

    pub fn supported_recipient_resolvers(&self) -> Vec<&'static str> {
        self.resolver_registry.supported_types()
    }
}
