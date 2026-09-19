// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod email;
mod lark;
mod slack;
mod webhook;

pub use email::{EMAIL_SMTP_CONNECTOR_TYPE, EmailSmtpConnectorAdapter};
pub use lark::{
    LARK_APP_CONNECTOR_TYPE, LARK_WEBHOOK_CONNECTOR_TYPE, LarkAppConnectorAdapter,
    LarkWebhookConnectorAdapter,
};
pub use slack::{
    SLACK_APP_CONNECTOR_TYPE, SLACK_WEBHOOK_CONNECTOR_TYPE, SlackAppConnectorAdapter,
    SlackWebhookConnectorAdapter,
};
pub use webhook::{WEBHOOK_CONNECTOR_TYPE, WebhookConnectorAdapter};
