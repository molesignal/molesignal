// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! AWS 短信：`SendTextMessage` 发一条事务类短信。不填发送方时由 AWS 按目的地选账号里的号码。

use std::time::Instant;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{Account, needs_the_connector, regions, send_message};
use crate::{
    domain::notify::connector::{
        ConnectorAdapter, ConnectorAddress, ConnectorCapabilities, ConnectorDeliveryResult,
        ConnectorRegion, NotifyChannel, NotifyConnector, NotifyMessage, NotifyTarget,
    },
    infra::notify::adapters::{common, phone},
    shared::Result,
};

pub const AWS_SMS_CONNECTOR_TYPE: &str = "aws_sms";

fn body(account: &Account, to: String, message: &NotifyMessage) -> Value {
    let mut body = json!({
        "DestinationPhoneNumber": to,
        "MessageBody": phone::texted(message),
        "MessageType": "TRANSACTIONAL",
    });

    if !account.origination_identity.trim().is_empty() {
        body["OriginationIdentity"] = json!(account.origination_identity.trim());
    }

    body
}

#[derive(Default)]
pub struct AwsSmsConnectorAdapter {
    /// 列区域用的 EC2 地址；空着是所在区域的 EC2 端点。
    ec2_base_url: Option<String>,
}

impl AwsSmsConnectorAdapter {
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
impl ConnectorAdapter for AwsSmsConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        AWS_SMS_CONNECTOR_TYPE
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            direct_user: true,
            ..ConnectorCapabilities::default()
        }
    }

    fn channel(&self) -> NotifyChannel {
        NotifyChannel::Sms
    }

    fn takes_replies(&self, config: &Value) -> bool {
        super::super::phone::replies_on(config)
    }

    fn addresses(&self) -> Vec<ConnectorAddress> {
        vec![super::address()]
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        Account::parse(config, AWS_SMS_CONNECTOR_TYPE)?.validate(config, AWS_SMS_CONNECTOR_TYPE)
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        phone::target_number(target, AWS_SMS_CONNECTOR_TYPE, phone::e164).map(|_| ())
    }

    async fn regions(
        &self,
        connector: &NotifyConnector,
        _language: &str,
    ) -> Result<Vec<ConnectorRegion>> {
        regions(
            AWS_SMS_CONNECTOR_TYPE,
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
        Err(needs_the_connector(AWS_SMS_CONNECTOR_TYPE))
    }

    async fn send_as(
        &self,
        connector: &NotifyConnector,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let account = Account::parse(&connector.config, AWS_SMS_CONNECTOR_TYPE)?;
        account.validate(&connector.config, AWS_SMS_CONNECTOR_TYPE)?;

        let to = phone::target_number(target, AWS_SMS_CONNECTOR_TYPE, phone::e164)?;
        let started = Instant::now();

        let message_id = send_message(
            AWS_SMS_CONNECTOR_TYPE,
            connector,
            &account,
            "SendTextMessage",
            &body(&account, to, message),
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
        matchers::{body_partial_json, body_string_contains, header, header_regex, method},
    };

    use super::{
        super::tests::{connector, key},
        *,
    };
    use crate::domain::notify::connector::NotifyTargetType;

    fn target() -> NotifyTarget {
        NotifyTarget {
            target_type: NotifyTargetType::FixedAddress,
            value: "+12065550123".into(),
            metadata: BTreeMap::new(),
        }
    }

    fn message() -> NotifyMessage {
        NotifyMessage {
            title: "CRITICAL · Checkout errors".into(),
            text: "Alert Checkout errors is open.".into(),
            markdown: None,
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: Default::default(),
            event: None,
            card: None,
        }
    }

    #[tokio::test]
    async fn a_text_is_transactional_and_says_the_title_first() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(header("x-amz-target", "PinpointSMSVoiceV2.SendTextMessage"))
            .and(body_partial_json(json!({
                "DestinationPhoneNumber": "+12065550123",
                "MessageBody": "CRITICAL · Checkout errors\nAlert Checkout errors is open.",
                "MessageType": "TRANSACTIONAL",
                "OriginationIdentity": "+12065550100",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"MessageId": "m-2"})))
            .expect(1)
            .mount(&server)
            .await;

        let result = AwsSmsConnectorAdapter::new()
            .send_as(
                &connector("aws-sms-1", key(&server.uri())),
                &target(),
                &message(),
            )
            .await
            .unwrap();

        assert_eq!(result.provider_message_id.as_deref(), Some("m-2"));
    }

    #[tokio::test]
    async fn a_refused_text_names_the_exception() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "__type": "com.amazonaws.pinpointsmsvoicev2#ValidationException",
                "Message": "The origination identity is not valid.",
            })))
            .mount(&server)
            .await;

        let error = AwsSmsConnectorAdapter::new()
            .send_as(
                &connector("aws-sms-2", key(&server.uri())),
                &target(),
                &message(),
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("HTTP 400 ValidationException"), "{error}");
    }

    #[tokio::test]
    async fn without_its_connector_it_does_not_send() {
        let adapter = AwsSmsConnectorAdapter::new();

        assert!(adapter.send(&key(""), &target(), &message()).await.is_err());
    }

    const REGIONS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DescribeRegionsResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
    <requestId>r-1</requestId>
    <regionInfo>
        <item><regionName>eu-west-1</regionName><regionEndpoint>ec2.eu-west-1.amazonaws.com</regionEndpoint></item>
        <item><regionName>ap-northeast-1</regionName><regionEndpoint>ec2.ap-northeast-1.amazonaws.com</regionEndpoint></item>
    </regionInfo>
</DescribeRegionsResponse>"#;

    #[tokio::test]
    async fn regions_are_the_ones_the_account_has() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(body_string_contains("Action=DescribeRegions"))
            .and(header_regex("authorization", "/us-east-1/ec2/aws4_request"))
            .respond_with(ResponseTemplate::new(200).set_body_string(REGIONS))
            .expect(1)
            .mount(&server)
            .await;

        let regions = AwsSmsConnectorAdapter::with_ec2_base_url(server.uri())
            .regions(&connector("aws-sms-4", key("")), "zh-cn")
            .await
            .unwrap();

        assert_eq!(
            regions
                .iter()
                .map(|region| region.id.as_str())
                .collect::<Vec<_>>(),
            ["ap-northeast-1", "eu-west-1"]
        );
        assert_eq!(
            regions[1].api_base_url,
            "https://sms-voice.eu-west-1.amazonaws.com"
        );
    }

    #[tokio::test]
    async fn without_leave_to_list_regions_it_names_the_permission() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403).set_body_string(
                r#"<?xml version="1.0" encoding="UTF-8"?><Response><Errors><Error><Code>UnauthorizedOperation</Code><Message>You are not authorized to perform this operation.</Message></Error></Errors><RequestID>r-2</RequestID></Response>"#,
            ))
            .mount(&server)
            .await;

        let error = AwsSmsConnectorAdapter::with_ec2_base_url(server.uri())
            .regions(&connector("aws-sms-5", key("")), "en-us")
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("ec2:DescribeRegions"), "{error}");
        assert!(error.contains("UnauthorizedOperation"), "{error}");
    }
}
