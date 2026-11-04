// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! AWS 语音：`SendVoiceMessage` 从一个能打电话的号码拨出，用 Amazon Polly 念通知，按设置重复几遍。

use std::time::Instant;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Account, needs_the_connector, regions, send_message};
use crate::{
    domain::notify::connector::{
        ConnectorAdapter, ConnectorAddress, ConnectorCapabilities, ConnectorDeliveryResult,
        ConnectorRegion, NotifyChannel, NotifyConnector, NotifyMessage, NotifyTarget,
    },
    infra::notify::adapters::{common, phone},
    shared::{Error, Result},
};

pub const AWS_VOICE_CONNECTOR_TYPE: &str = "aws_voice";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VoiceConfig {
    #[serde(flatten)]
    account: Account,
    #[serde(default)]
    voice_id: String,
    #[serde(default = "default_play_times")]
    play_times: u8,
}

fn default_play_times() -> u8 {
    2
}

impl VoiceConfig {
    fn parse(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|error| {
            Error::invalid(format!(
                "invalid {AWS_VOICE_CONNECTOR_TYPE} config: {error}"
            ))
        })
    }

    fn validate(&self, config: &Value) -> Result<()> {
        self.account.validate(config, AWS_VOICE_CONNECTOR_TYPE)?;

        common::require(
            &self.account.origination_identity,
            "aws_voice originationIdentity",
        )?;

        if !self
            .voice_id
            .trim()
            .chars()
            .all(|c| c.is_ascii_alphabetic())
        {
            return Err(Error::invalid(
                "aws_voice voice_id must be an Amazon Polly voice, like JOANNA or ZHIYU",
            ));
        }

        if !(1..=3).contains(&self.play_times) {
            return Err(Error::invalid(
                "aws_voice play_times must be between 1 and 3",
            ));
        }

        Ok(())
    }

    fn body(&self, to: String, message: &NotifyMessage) -> Value {
        let said = phone::spoken(message);
        let repeated = vec![said; usize::from(self.play_times)].join(" … ");

        let mut body = json!({
            "DestinationPhoneNumber": to,
            "OriginationIdentity": self.account.origination_identity.trim(),
            "MessageBody": repeated,
            "MessageBodyTextType": "TEXT",
        });

        if !self.voice_id.trim().is_empty() {
            body["VoiceId"] = json!(self.voice_id.trim().to_ascii_uppercase());
        }

        body
    }
}

#[derive(Default)]
pub struct AwsVoiceConnectorAdapter {
    /// 列区域用的 EC2 地址；空着是所在区域的 EC2 端点。
    ec2_base_url: Option<String>,
}

impl AwsVoiceConnectorAdapter {
    pub const fn new() -> Self {
        Self { ec2_base_url: None }
    }

    #[cfg(test)]
    fn with_ec2_base_url(ec2_base_url: String) -> Self {
        Self {
            ec2_base_url: Some(ec2_base_url),
        }
    }
}

#[async_trait]
impl ConnectorAdapter for AwsVoiceConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        AWS_VOICE_CONNECTOR_TYPE
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            direct_user: true,
            ..ConnectorCapabilities::default()
        }
    }

    fn channel(&self) -> NotifyChannel {
        NotifyChannel::Phone
    }

    fn addresses(&self) -> Vec<ConnectorAddress> {
        vec![super::address()]
    }

    fn merge_window_secs(&self) -> Option<u32> {
        Some(phone::CALL_MERGE_WINDOW_SECS)
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        VoiceConfig::parse(config)?.validate(config)
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        phone::target_number(target, AWS_VOICE_CONNECTOR_TYPE, phone::e164).map(|_| ())
    }

    async fn regions(
        &self,
        connector: &NotifyConnector,
        _language: &str,
    ) -> Result<Vec<ConnectorRegion>> {
        regions(
            AWS_VOICE_CONNECTOR_TYPE,
            connector,
            self.ec2_base_url.as_deref(),
        )
        .await
    }

    async fn send(
        &self,
        _config: &Value,
        _target: &NotifyTarget,
        _message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        Err(needs_the_connector(AWS_VOICE_CONNECTOR_TYPE))
    }

    async fn send_as(
        &self,
        connector: &NotifyConnector,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = VoiceConfig::parse(&connector.config)?;
        config.validate(&connector.config)?;

        let to = phone::target_number(target, AWS_VOICE_CONNECTOR_TYPE, phone::e164)?;
        let started = Instant::now();

        let message_id = send_message(
            AWS_VOICE_CONNECTOR_TYPE,
            connector,
            &config.account,
            "SendVoiceMessage",
            &config.body(to, message),
        )
        .await?;

        Ok(common::delivered(message_id, started))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, header, header_exists, method, path},
    };

    use super::{
        super::tests::{connector, key},
        *,
    };
    use crate::domain::notify::connector::NotifyTargetType;

    #[tokio::test]
    async fn a_call_is_read_twice_by_the_chosen_voice() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/"))
            .and(header(
                "x-amz-target",
                "PinpointSMSVoiceV2.SendVoiceMessage",
            ))
            .and(header("content-type", "application/x-amz-json-1.0"))
            .and(header_exists("authorization"))
            .and(body_json(json!({
                "DestinationPhoneNumber": "+12065550123",
                "OriginationIdentity": "+12065550100",
                "MessageBody": "Disk full … Disk full",
                "MessageBodyTextType": "TEXT",
                "VoiceId": "JOANNA",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"MessageId": "m-1"})))
            .expect(1)
            .mount(&server)
            .await;

        let mut config = key(&server.uri());
        config["voiceId"] = json!("joanna");

        let target = NotifyTarget {
            target_type: NotifyTargetType::DirectUser,
            value: "+12065550123".into(),
            metadata: BTreeMap::new(),
        };

        let message = NotifyMessage {
            title: "Disk full".into(),
            text: String::new(),
            markdown: None,
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: Default::default(),
            event: None,
            card: None,
        };

        let result = AwsVoiceConnectorAdapter::new()
            .send_as(&connector("aws-voice-1", config), &target, &message)
            .await
            .unwrap();

        assert_eq!(result.provider_message_id.as_deref(), Some("m-1"));
    }

    #[test]
    fn a_call_needs_a_number_to_call_from() {
        let mut config = key("");
        config["originationIdentity"] = json!("");

        assert!(
            AwsVoiceConnectorAdapter::new()
                .validate_config(&config)
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_voice_connector_lists_regions_from_the_same_account_call() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(header_exists("authorization"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "<DescribeRegionsResponse><regionInfo><item><regionName>us-west-2</regionName></item></regionInfo></DescribeRegionsResponse>",
            ))
            .expect(1)
            .mount(&server)
            .await;

        let regions = AwsVoiceConnectorAdapter::with_ec2_base_url(server.uri())
            .regions(&connector("aws-voice-9", key("")), "en-us")
            .await
            .unwrap();

        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].id, "us-west-2");
    }
}
