// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Amazon SNS: `Publish` to a topic through the SNS Query API, signed with SigV4 like the other
//! AWS connectors. The message is what the generic webhook sends: the machine-readable event by
//! default, or the body of the notify template (`body`). The title becomes the subject as SNS takes
//! one, and the event type and the alert's severity go along as message attributes for
//! subscription filter policies. A FIFO topic orders messages by alert and drops a delivery sent
//! twice.

use std::time::Instant;

use async_trait::async_trait;
use domain::notify::connector::{
    ConnectorAdapter, ConnectorAddress, ConnectorCapabilities, ConnectorDeliveryResult,
    ConnectorRegion, NotifyChannel, NotifyConnector, NotifyMessage, NotifyTarget, RegionHost,
};
use kernel::{Error, Result, ids::Id};
use serde::Deserialize;
use serde_json::Value;
use url::form_urlencoded;

use super::{Account, needs_the_connector, regions, signed_post, well_formed_region};
use crate::infra::{
    connectors::aws as aws_identity,
    notify::adapters::{
        common,
        webhook::{self, WebhookBody, WebhookConnectorAdapter},
    },
};

pub const AWS_SNS_CONNECTOR_TYPE: &str = "aws_sns";

/// The SigV4 service name, which is also the first label of the endpoint's host.
const SERVICE: &str = "sns";

const API_VERSION: &str = "2010-03-31";

const FORM: &str = "application/x-www-form-urlencoded; charset=utf-8";

/// SNS takes a subject of fewer than 100 characters.
const MAX_SUBJECT_CHARS: usize = 99;

/// SNS takes up to 128 characters in a message group or deduplication ID.
const MAX_FIFO_ID_CHARS: usize = 128;

/// The AWS settings every AWS notify connector has, the topic, and what the message is.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnsConfig {
    #[serde(flatten)]
    account: Account,
    #[serde(default)]
    topic_arn: String,
    #[serde(default)]
    body: WebhookBody,
}

impl SnsConfig {
    fn parse(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|error| {
            Error::invalid(format!("invalid {AWS_SNS_CONNECTOR_TYPE} config: {error}"))
        })
    }

    /// `config` is the whole configuration: the credentials are read from it.
    fn validate(&self, config: &Value) -> Result<()> {
        self.account.validate(config, AWS_SNS_CONNECTOR_TYPE)?;

        let Some(region) = topic_region(self.topic_arn.trim()) else {
            return Err(Error::invalid(
                "aws_sns topicArn must be a topic's ARN, like arn:aws:sns:us-east-1:123456789012:alerts",
            ));
        };

        if region != self.account.region.trim() {
            return Err(Error::invalid(format!(
                "aws_sns topicArn is a topic in {region}: SNS publishes only in the topic's region, so the region must be {region}"
            )));
        }

        Ok(())
    }

    fn fifo(&self) -> bool {
        self.topic_arn.trim().ends_with(".fifo")
    }
}

/// The region of a topic ARN, `arn:<partition>:sns:<region>:<account>:<topic>`, or `None` when the
/// ARN is not a topic's. The account is 12 digits; the topic's name is letters, digits, `_` and
/// `-`, and a FIFO topic's ends in `.fifo`.
fn topic_region(arn: &str) -> Option<&str> {
    let parts = arn.split(':').collect::<Vec<_>>();

    let &[scheme, partition, service, region, account, topic] = parts.as_slice() else {
        return None;
    };

    let name = topic.strip_suffix(".fifo").unwrap_or(topic);

    let well_formed = scheme == "arn"
        && partition.starts_with("aws")
        && service == SERVICE
        && well_formed_region(region)
        && account.len() == 12
        && account.bytes().all(|byte| byte.is_ascii_digit())
        && topic.len() <= 256
        && !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'));

    well_formed.then_some(region)
}

/// The host of the SNS endpoint in `region`; the China regions are in the second partition.
fn sns_host(region: &str) -> String {
    let domain = if region.starts_with("cn-") {
        aws_identity::PARTITION_DOMAINS[1]
    } else {
        aws_identity::PARTITION_DOMAINS[0]
    };

    format!("{SERVICE}.{region}.{domain}")
}

/// The `Publish` request as a form: the topic, the message with its subject and attributes, and
/// on a FIFO topic the message group and the deduplication ID.
fn publish_form(
    config: &SnsConfig,
    target: &NotifyTarget,
    message: &NotifyMessage,
) -> Result<String> {
    let body = match config.body {
        WebhookBody::Event => webhook::event_body(target, message)?.to_string(),
        WebhookBody::Template => message.text.clone(),
    };

    if body.trim().is_empty() {
        return Err(Error::invalid(
            "aws_sns has nothing to publish: the body of the notify template is empty",
        ));
    }

    let mut form = form_urlencoded::Serializer::new(String::new());

    form.append_pair("Action", "Publish")
        .append_pair("Version", API_VERSION)
        .append_pair("TopicArn", config.topic_arn.trim())
        .append_pair("Message", &body);

    if let Some(subject) = subject(&message.title) {
        form.append_pair("Subject", &subject);
    }

    for (index, (name, value)) in attributes(message).into_iter().enumerate() {
        let entry = format!("MessageAttributes.entry.{}", index + 1);

        form.append_pair(&format!("{entry}.Name"), name)
            .append_pair(&format!("{entry}.Value.DataType"), "String")
            .append_pair(&format!("{entry}.Value.StringValue"), &value);
    }

    if config.fifo() {
        let (group, deduplication) = fifo_ids(message);

        form.append_pair("MessageGroupId", &group)
            .append_pair("MessageDeduplicationId", &deduplication);
    }

    Ok(form.finish())
}

/// The subject as SNS takes it: one line without control characters, fewer than 100 characters.
/// `None` when the title is empty.
fn subject(title: &str) -> Option<String> {
    let line = title
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    (!line.is_empty()).then(|| common::truncate_chars(&line, MAX_SUBJECT_CHARS))
}

/// The attributes subscription filter policies match on: the event type and, for an alert, its
/// severity. SNS refuses an empty attribute, so one the message lacks is left out.
fn attributes(message: &NotifyMessage) -> Vec<(&'static str, String)> {
    let severity = message
        .metadata
        .get("severity")
        .map(String::as_str)
        .or_else(|| event_text(message, "/alert/severity"));

    [
        ("event_type", webhook::event_type(message)),
        ("severity", severity),
    ]
    .into_iter()
    .filter_map(|(name, value)| {
        let value = value?.trim();
        (!value.is_empty()).then(|| (name, value.to_string()))
    })
    .collect()
}

/// A FIFO topic's message group, which keeps an alert's messages in order (other events are
/// grouped by type), and the deduplication ID: the delivery's, or a new one for a test.
fn fifo_ids(message: &NotifyMessage) -> (String, String) {
    let group = message
        .metadata
        .get("alert_id")
        .map(String::as_str)
        .or_else(|| event_text(message, "/alert/id"))
        .or_else(|| webhook::event_type(message))
        .map(fifo_id)
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| "molesignal".into());

    let deduplication = message
        .metadata
        .get("delivery_id")
        .map(String::as_str)
        .map(fifo_id)
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| Id::new().to_string());

    (group, deduplication)
}

/// SNS takes letters, digits and ASCII punctuation in a group or deduplication ID.
fn fifo_id(value: &str) -> String {
    value
        .chars()
        .filter(char::is_ascii_graphic)
        .take(MAX_FIFO_ID_CHARS)
        .collect()
}

/// A text in the event the notify engine wrote for machines.
fn event_text<'a>(message: &'a NotifyMessage, pointer: &str) -> Option<&'a str> {
    message.event.as_ref()?.pointer(pointer)?.as_str()
}

/// Publishes the request as `connector` and returns the message's ID.
async fn publish(
    connector: &NotifyConnector,
    config: &SnsConfig,
    form: String,
) -> Result<Option<String>> {
    let response = signed_post(
        AWS_SNS_CONNECTOR_TYPE,
        connector,
        &config.account,
        &config.account.endpoint_at(sns_host),
        SERVICE,
        &[("content-type", FORM)],
        form,
    )
    .await?;

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let document = roxmltree::Document::parse(&text).ok();

    // The text of the first element called `name`.
    let first = |name: &str| {
        document
            .as_ref()
            .and_then(|document| {
                document
                    .descendants()
                    .find(|node| node.has_tag_name(name))
                    .and_then(|node| node.text())
            })
            .map(str::trim)
            .unwrap_or_default()
            .to_string()
    };

    if status.is_success() {
        let message_id = first("MessageId");
        return Ok((!message_id.is_empty()).then_some(message_id));
    }

    let code = match first("Code") {
        code if code.is_empty() => "error".to_string(),
        code => code,
    };

    let refused = format!(
        "HTTP {} {code}: {}",
        status.as_u16(),
        common::provider_message(&first("Message"))
    );

    if status.as_u16() == 429 || code.contains("Throttl") {
        return Err(Error::unavailable(format!(
            "{AWS_SNS_CONNECTOR_TYPE} was throttled by AWS ({refused}); retry later"
        )));
    }

    Err(Error::unavailable(format!(
        "{AWS_SNS_CONNECTOR_TYPE} was refused: {refused}"
    )))
}

#[derive(Default)]
pub struct AwsSnsConnectorAdapter {
    /// The EC2 address regions are listed from; empty is the region's EC2 endpoint.
    ec2_base_url: Option<String>,
}

impl AwsSnsConnectorAdapter {
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
impl ConnectorAdapter for AwsSnsConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        AWS_SNS_CONNECTOR_TYPE
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            direct_user: true,
            group: true,
            ..ConnectorCapabilities::default()
        }
    }

    fn channel(&self) -> NotifyChannel {
        NotifyChannel::Webhook
    }

    /// The region's SNS endpoint, `https://sns.{region}.{partition's domain}`.
    fn addresses(&self) -> Vec<ConnectorAddress> {
        vec![ConnectorAddress::by_region(
            "apiBaseUrl",
            RegionHost {
                service: SERVICE,
                domains: &aws_identity::PARTITION_DOMAINS,
            },
            None,
        )]
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        SnsConfig::parse(config)?.validate(config)
    }

    /// Messages go to the connector's topic: a target is taken as the generic webhook takes one,
    /// and the event body names it.
    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        WebhookConnectorAdapter::new().validate_target(target)
    }

    async fn regions(
        &self,
        connector: &NotifyConnector,
        _language: &str,
    ) -> Result<Vec<ConnectorRegion>> {
        let regions = regions(
            AWS_SNS_CONNECTOR_TYPE,
            connector,
            self.ec2_base_url.as_deref(),
        )
        .await?;

        Ok(regions
            .into_iter()
            .map(|region| ConnectorRegion {
                api_base_url: format!("https://{}", sns_host(&region.id)),
                ..region
            })
            .collect())
    }

    async fn send(
        &self,
        _config: &Value,
        _target: &NotifyTarget,
        _message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        Err(needs_the_connector(AWS_SNS_CONNECTOR_TYPE))
    }

    async fn send_as(
        &self,
        connector: &NotifyConnector,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = SnsConfig::parse(&connector.config)?;
        config.validate(&connector.config)?;
        self.validate_target(target)?;

        let form = publish_form(&config, target, message)?;
        let started = Instant::now();
        let message_id = publish(connector, &config, form).await?;

        Ok(common::delivered(message_id, started))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use domain::notify::connector::NotifyTargetType;
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_string_contains, header, header_regex, method},
    };

    use super::{
        super::tests::{connector, key},
        *,
    };

    const TOPIC: &str = "arn:aws:sns:us-east-1:123456789012:molesignal-alerts";

    const PUBLISHED: &str = r#"<PublishResponse xmlns="https://sns.amazonaws.com/doc/2010-03-31/">
    <PublishResult><MessageId>567910cd-659e-55d4-8ccb-5aaf14679dc0</MessageId></PublishResult>
    <ResponseMetadata><RequestId>d74b8436-ae13-5ab4-a9ff-ce54dfea72a0</RequestId></ResponseMetadata>
</PublishResponse>"#;

    fn config(base: &str, topic: &str) -> Value {
        let mut config = key(base);
        config["topicArn"] = json!(topic);
        config
    }

    fn target() -> NotifyTarget {
        NotifyTarget {
            target_type: NotifyTargetType::FixedGroup,
            value: "platform-oncall".into(),
            metadata: BTreeMap::new(),
        }
    }

    fn message() -> NotifyMessage {
        NotifyMessage {
            title: "CRITICAL · Checkout errors\r\nrose\tabove 5%".into(),
            text: r#"{"summary":"Checkout errors"}"#.into(),
            markdown: None,
            html: None,
            metadata: [
                ("alert_id", "inc-1"),
                ("event_type", "alert.triggered"),
                ("severity", "critical"),
                ("delivery_id", "dlv-1"),
            ]
            .into_iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect(),
            provider_templates: Default::default(),
            event: Some(json!({
                "version": 2,
                "event": {"id": "alert.triggered:inc-1", "type": "alert.triggered"},
                "alert": {"id": "inc-1", "severity": "critical"},
            })),
            card: None,
        }
    }

    /// The form of the one request the server received.
    async fn published(server: &MockServer) -> BTreeMap<String, String> {
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);

        form_urlencoded::parse(&requests[0].body)
            .into_owned()
            .collect()
    }

    async fn publishing(server: &MockServer) {
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string(PUBLISHED))
            .expect(1)
            .mount(server)
            .await;
    }

    #[test]
    fn the_topic_is_one_in_the_connectors_region() {
        let adapter = AwsSnsConnectorAdapter::new();
        let valid = config("", TOPIC);

        assert!(adapter.validate_config(&valid).is_ok());

        let mut template = valid.clone();
        template["body"] = json!("template");
        assert!(adapter.validate_config(&template).is_ok());

        let mut unknown_body = valid.clone();
        unknown_body["body"] = json!("xml");
        assert!(adapter.validate_config(&unknown_body).is_err());

        for topic in [
            "",
            "arn:aws:sqs:us-east-1:123456789012:alerts",
            "arn:aws:sns:us-east-1:1234:alerts",
            "arn:aws:sns:us-east-1:123456789012:",
            "arn:aws:sns:us-east-1:123456789012:alerts/x",
        ] {
            assert!(
                adapter.validate_config(&config("", topic)).is_err(),
                "{topic}"
            );
        }

        let elsewhere = adapter
            .validate_config(&config("", "arn:aws:sns:eu-west-1:123456789012:alerts"))
            .unwrap_err()
            .to_string();
        assert!(elsewhere.contains("eu-west-1"), "{elsewhere}");

        let mut china = config("", "arn:aws-cn:sns:cn-north-1:123456789012:alerts.fifo");
        china["region"] = json!("cn-north-1");
        assert!(adapter.validate_config(&china).is_ok());

        let china = SnsConfig::parse(&china).unwrap();
        assert!(china.fifo());
        assert_eq!(
            china.account.endpoint_at(sns_host),
            "https://sns.cn-north-1.amazonaws.com.cn/"
        );
    }

    #[test]
    fn the_subject_is_one_short_line() {
        assert_eq!(
            subject("CRITICAL · Checkout errors\r\nrose\u{7}  above 5%").as_deref(),
            Some("CRITICAL · Checkout errors rose above 5%")
        );
        assert_eq!(
            subject(&"x".repeat(300)).unwrap().chars().count(),
            MAX_SUBJECT_CHARS
        );
        assert_eq!(subject(" \n "), None);
    }

    #[test]
    fn a_fifo_message_without_alert_or_delivery_still_has_its_ids() {
        let mut test = message();
        test.metadata.clear();
        test.event = None;

        let (group, deduplication) = fifo_ids(&test);
        assert_eq!(group, "molesignal");
        assert!(!deduplication.is_empty());

        test.metadata
            .insert("event_type".into(), "oncall.shift.starting".into());
        assert_eq!(fifo_ids(&test).0, "oncall.shift.starting");
        assert_eq!(fifo_id(&"é1 2".repeat(100)).len(), MAX_FIFO_ID_CHARS);
    }

    #[tokio::test]
    async fn an_event_is_published_signed_with_its_subject_and_attributes() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(header("content-type", FORM))
            .and(header_regex("authorization", "/us-east-1/sns/aws4_request"))
            .and(body_string_contains("Action=Publish"))
            .respond_with(ResponseTemplate::new(200).set_body_string(PUBLISHED))
            .expect(1)
            .mount(&server)
            .await;

        let result = AwsSnsConnectorAdapter::new()
            .send_as(
                &connector("aws-sns-1", config(&server.uri(), TOPIC)),
                &target(),
                &message(),
            )
            .await
            .unwrap();

        assert_eq!(
            result.provider_message_id.as_deref(),
            Some("567910cd-659e-55d4-8ccb-5aaf14679dc0")
        );

        let form = published(&server).await;
        assert_eq!(form["Version"], "2010-03-31");
        assert_eq!(form["TopicArn"], TOPIC);
        assert_eq!(form["Subject"], "CRITICAL · Checkout errors rose above 5%");
        assert_eq!(form["MessageAttributes.entry.1.Name"], "event_type");
        assert_eq!(form["MessageAttributes.entry.1.Value.DataType"], "String");
        assert_eq!(
            form["MessageAttributes.entry.1.Value.StringValue"],
            "alert.triggered"
        );
        assert_eq!(form["MessageAttributes.entry.2.Name"], "severity");
        assert_eq!(
            form["MessageAttributes.entry.2.Value.StringValue"],
            "critical"
        );
        assert!(!form.contains_key("MessageGroupId"));

        // The same JSON the generic webhook sends.
        let body: Value = serde_json::from_str(&form["Message"]).unwrap();
        assert_eq!(body["event"]["type"], "alert.triggered");
        assert_eq!(body["alert"]["severity"], "critical");
        assert_eq!(body["target"]["value"], "platform-oncall");
        assert_eq!(body["message"]["metadata"]["alert_id"], "inc-1");
        assert!(body["message"]["metadata"].get("delivery_id").is_none());
    }

    #[tokio::test]
    async fn a_fifo_topic_keeps_an_alert_in_order_and_a_delivery_once() {
        let server = MockServer::start().await;
        publishing(&server).await;

        let fifo = "arn:aws:sns:us-east-1:123456789012:molesignal-alerts.fifo";

        AwsSnsConnectorAdapter::new()
            .send_as(
                &connector("aws-sns-2", config(&server.uri(), fifo)),
                &target(),
                &message(),
            )
            .await
            .unwrap();

        let form = published(&server).await;
        assert_eq!(form["MessageGroupId"], "inc-1");
        assert_eq!(form["MessageDeduplicationId"], "dlv-1");
    }

    #[tokio::test]
    async fn the_template_body_is_published_as_written() {
        let server = MockServer::start().await;
        publishing(&server).await;

        let mut template = config(&server.uri(), TOPIC);
        template["body"] = json!("template");

        let mut message = message();
        message.title = String::new();
        message.metadata.clear();

        AwsSnsConnectorAdapter::new()
            .send_as(&connector("aws-sns-3", template), &target(), &message)
            .await
            .unwrap();

        let form = published(&server).await;
        assert_eq!(form["Message"], r#"{"summary":"Checkout errors"}"#);
        assert!(!form.contains_key("Subject"));

        // Without metadata the attributes come from the event.
        assert_eq!(
            form["MessageAttributes.entry.1.Value.StringValue"],
            "alert.triggered"
        );
        assert_eq!(
            form["MessageAttributes.entry.2.Value.StringValue"],
            "critical"
        );
    }

    #[tokio::test]
    async fn a_refusal_names_the_error_and_throttling_asks_to_retry() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(body_string_contains("molesignal-alerts"))
            .respond_with(ResponseTemplate::new(404).set_body_string(
                r#"<ErrorResponse xmlns="https://sns.amazonaws.com/doc/2010-03-31/"><Error><Type>Sender</Type><Code>NotFound</Code><Message>Topic does not exist</Message></Error><RequestId>r-1</RequestId></ErrorResponse>"#,
            ))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(body_string_contains("busy-topic"))
            .respond_with(ResponseTemplate::new(400).set_body_string(
                r#"<ErrorResponse><Error><Type>Sender</Type><Code>Throttling</Code><Message>Rate exceeded</Message></Error></ErrorResponse>"#,
            ))
            .mount(&server)
            .await;

        let adapter = AwsSnsConnectorAdapter::new();

        let missing = adapter
            .send_as(
                &connector("aws-sns-4", config(&server.uri(), TOPIC)),
                &target(),
                &message(),
            )
            .await
            .unwrap_err();

        assert!(matches!(missing, Error::Unavailable(_)), "{missing}");
        assert!(
            missing
                .to_string()
                .contains("HTTP 404 NotFound: Topic does not exist"),
            "{missing}"
        );

        let busy = "arn:aws:sns:us-east-1:123456789012:busy-topic";

        let throttled = adapter
            .send_as(
                &connector("aws-sns-5", config(&server.uri(), busy)),
                &target(),
                &message(),
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(throttled.contains("Throttling"), "{throttled}");
        assert!(throttled.contains("retry later"), "{throttled}");
    }

    #[tokio::test]
    async fn without_its_connector_it_does_not_publish() {
        let adapter = AwsSnsConnectorAdapter::new();

        assert!(
            adapter
                .send(&config("", TOPIC), &target(), &message())
                .await
                .is_err()
        );
    }

    const REGIONS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DescribeRegionsResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
    <regionInfo>
        <item><regionName>eu-west-1</regionName></item>
        <item><regionName>us-east-1</regionName></item>
    </regionInfo>
</DescribeRegionsResponse>"#;

    #[tokio::test]
    async fn each_region_offers_its_sns_endpoint() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(body_string_contains("Action=DescribeRegions"))
            .respond_with(ResponseTemplate::new(200).set_body_string(REGIONS))
            .expect(1)
            .mount(&server)
            .await;

        let regions = AwsSnsConnectorAdapter::with_ec2_base_url(server.uri())
            .regions(&connector("aws-sns-6", config("", TOPIC)), "en-us")
            .await
            .unwrap();

        assert_eq!(
            regions
                .iter()
                .map(|region| region.api_base_url.as_str())
                .collect::<Vec<_>>(),
            [
                "https://sns.eu-west-1.amazonaws.com",
                "https://sns.us-east-1.amazonaws.com"
            ]
        );
    }
}
