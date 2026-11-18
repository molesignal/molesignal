// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 通知的两张注册表：各种连接器怎样发出去，收件人怎样解析成人。

use std::sync::Arc;

use agent::im::ImRepository;
use domain::{
    alerting::repositories::ScheduleRepository,
    iam::{IamMembershipRepository, TeamRepository},
    notify::{connector::ConnectorAdapter, recipient::RecipientResolver},
};
use kernel::Result;

use crate::{
    app::notify::{
        AlertOwnerResolver, ConnectorRegistry, CurrentOncallResolver, EventUsersResolver,
        FixedUsersResolver, NextOncallResolver, RecipientResolverRegistry, ScheduleMembersResolver,
        TeamLeadResolver, TeamMembersResolver,
    },
    infra::notify::adapters::{
        AliyunSmsConnectorAdapter, AliyunVoiceConnectorAdapter, AwsSmsConnectorAdapter,
        AwsSnsConnectorAdapter, AwsVoiceConnectorAdapter, DingTalkAppConnectorAdapter,
        DingTalkWebhookConnectorAdapter, DiscordWebhookConnectorAdapter, EmailSmtpConnectorAdapter,
        GoogleChatAppConnectorAdapter, GoogleChatWebhookConnectorAdapter, HttpSmsConnectorAdapter,
        HuaweiSmsConnectorAdapter, HuaweiVoiceConnectorAdapter, LarkAppConnectorAdapter,
        LarkWebhookConnectorAdapter, MatrixBotConnectorAdapter, MattermostBotConnectorAdapter,
        MattermostWebhookConnectorAdapter, PagerDutyConnectorAdapter, SlackAppConnectorAdapter,
        SlackWebhookConnectorAdapter, TeamsAppConnectorAdapter, TeamsWebhookConnectorAdapter,
        TelegramBotConnectorAdapter, TencentSmsConnectorAdapter, TencentVoiceConnectorAdapter,
        TwilioSmsConnectorAdapter, TwilioVoiceConnectorAdapter, TwilioWhatsappConnectorAdapter,
        WebexBotConnectorAdapter, WebexWebhookConnectorAdapter, WebhookConnectorAdapter,
        WecomAppConnectorAdapter, WecomBotConnectorAdapter, WecomWebhookConnectorAdapter,
    },
};

/// `im` 是 Mole Agent 在 IM 里的记录：企业微信智能机器人的通知在本节点没连着机器人时记进它的待发回复。
pub(super) fn connector_registry(im: Arc<dyn ImRepository>) -> Result<Arc<ConnectorRegistry>> {
    let connector_adapters: Vec<Arc<dyn ConnectorAdapter>> = vec![
        Arc::new(EmailSmtpConnectorAdapter::new()),
        Arc::new(SlackAppConnectorAdapter::new()),
        Arc::new(SlackWebhookConnectorAdapter::new()),
        Arc::new(LarkAppConnectorAdapter::new()),
        Arc::new(LarkWebhookConnectorAdapter::new()),
        Arc::new(DingTalkAppConnectorAdapter::new()),
        Arc::new(DingTalkWebhookConnectorAdapter::new()),
        Arc::new(WecomAppConnectorAdapter::new()),
        Arc::new(WecomWebhookConnectorAdapter::new()),
        Arc::new(WecomBotConnectorAdapter::with_outbox(im)),
        Arc::new(TeamsAppConnectorAdapter::new()),
        Arc::new(TeamsWebhookConnectorAdapter::new()),
        Arc::new(GoogleChatAppConnectorAdapter::new()),
        Arc::new(GoogleChatWebhookConnectorAdapter::new()),
        Arc::new(TelegramBotConnectorAdapter::new()),
        Arc::new(DiscordWebhookConnectorAdapter::new()),
        Arc::new(MattermostBotConnectorAdapter::new()),
        Arc::new(MattermostWebhookConnectorAdapter::new()),
        Arc::new(WebexBotConnectorAdapter::new()),
        Arc::new(WebexWebhookConnectorAdapter::new()),
        Arc::new(MatrixBotConnectorAdapter::new()),
        Arc::new(AliyunVoiceConnectorAdapter::new()),
        Arc::new(AliyunSmsConnectorAdapter::new()),
        Arc::new(TencentVoiceConnectorAdapter::new()),
        Arc::new(TencentSmsConnectorAdapter::new()),
        Arc::new(HuaweiVoiceConnectorAdapter::new()),
        Arc::new(HuaweiSmsConnectorAdapter::new()),
        Arc::new(TwilioVoiceConnectorAdapter::new()),
        Arc::new(TwilioSmsConnectorAdapter::new()),
        Arc::new(TwilioWhatsappConnectorAdapter::new()),
        Arc::new(AwsVoiceConnectorAdapter::new()),
        Arc::new(AwsSmsConnectorAdapter::new()),
        Arc::new(HttpSmsConnectorAdapter::new()),
        Arc::new(AwsSnsConnectorAdapter::new()),
        Arc::new(PagerDutyConnectorAdapter::new()),
        Arc::new(WebhookConnectorAdapter::new()),
    ];

    Ok(Arc::new(ConnectorRegistry::new(connector_adapters)?))
}

pub(super) fn resolver_registry(
    memberships: Arc<dyn IamMembershipRepository>,
    schedules: Arc<dyn ScheduleRepository>,
    teams: Arc<dyn TeamRepository>,
) -> Result<Arc<RecipientResolverRegistry>> {
    let recipient_resolvers: Vec<Arc<dyn RecipientResolver>> = vec![
        Arc::new(FixedUsersResolver::new(memberships.clone())),
        Arc::new(CurrentOncallResolver::new(schedules.clone())),
        Arc::new(NextOncallResolver::new(schedules.clone())),
        Arc::new(ScheduleMembersResolver::new(schedules.clone())),
        Arc::new(TeamMembersResolver::new(teams.clone())),
        Arc::new(TeamLeadResolver::new(teams.clone())),
        Arc::new(EventUsersResolver::new(memberships.clone())),
        Arc::new(AlertOwnerResolver::new(memberships)),
    ];

    Ok(Arc::new(RecipientResolverRegistry::new(
        recipient_resolvers,
    )?))
}
