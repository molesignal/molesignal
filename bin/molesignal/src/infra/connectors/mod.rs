// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cloud connectors。
//!
//! 按厂商分目录：[`aws`]、[`azure`]、[`gcp`]、[`aliyun`]、[`tencent`]、[`huawei`]、[`cloudflare`]、
//! [`heroku`]；不属于某个厂商的放在 [`sources`]（Kafka、S3）、[`cloud_metrics`]（指标拉取的窗口与
//! 命名）、[`cloud_events`]（云事件总线的推送）。
//!
//! 来源（ingress）：
//! - 拉取：[`aws::cloudwatch_logs`]，[`sources::kafka`]、[`sources::s3`]（读取位点存在
//!   `connectors.read_state`，写入成功后才前进），以及云监控指标 [`aws::cloudwatch_metrics`]、
//!   [`azure::monitor`]、[`gcp::monitoring`]、[`aliyun::cms`]、[`tencent::monitor`]、[`huawei::ces`]
//!   （按周期拉取聚合值，共用 [`cloud_metrics`]，写入 `metricsStream`）
//! - 推送：[`aws::kinesis_firehose`]（日志与 CloudWatch Metric Streams 的指标）、[`gcp::pubsub`]、
//!   [`cloud_events`]、[`cloudflare`]、[`heroku`]，HTTP 端点在 `api::http::routes::intake::connectors`
//!
//! 目标（egress）：S3、Kafka、HTTP，见 [`dispatcher`]。`ConnectorRunner` 在 `connector` role 内调度
//! pull；push connector 由 HTTP layer 直接走。[`check`] 在保存前试连一个 connector。
//!
//! 配置（`config_json`）按 `kind` 各自定义（见 [`config`]），密钥落库时由 [`secrets`] 封存；
//! 来源共有的字段：`targetStream`（Firehose 另有写指标的 `metricsStream`）。

use async_trait::async_trait;
use domain::intake::RawEvent;
use kernel::{Error, Result, ids::Id};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod aliyun;
pub mod aws;
pub mod azure;
pub mod check;
pub mod cloud_events;
pub mod cloud_metrics;
pub mod cloudflare;
pub mod config;
pub mod dispatcher;
pub mod enrich;
pub mod gcp;
pub mod heroku;
pub mod huawei;
pub mod repo;
pub mod runner;
pub mod secrets;
pub mod sources;
pub mod tencent;

pub use dispatcher::{ConnectorDispatcher, EgressDispatcher};
pub use repo::{Connector, ConnectorRepository, PgConnectorRepository};
pub use runner::ConnectorRunner;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorKind {
    AwsCloudwatchLogs,
    AwsKinesisFirehose,
    GcpPubsubPush,
    GcpPubsubPull,
    CloudEvents,
    CloudflareLogpush,
    HerokuDrain,
    S3,
    Kafka,
    Http,
    KafkaSource,
    S3Source,
    AwsCloudwatchMetrics,
    AzureMonitorMetrics,
    GcpMonitoringMetrics,
    AliyunCmsMetrics,
    TencentMonitorMetrics,
    HuaweiCesMetrics,
}

impl ConnectorKind {
    /// Whether the runner pulls this kind on a timer.
    pub fn is_pull(&self) -> bool {
        matches!(
            self,
            Self::AwsCloudwatchLogs
                | Self::KafkaSource
                | Self::S3Source
                | Self::GcpPubsubPull
                | Self::AwsCloudwatchMetrics
                | Self::AzureMonitorMetrics
                | Self::GcpMonitoringMetrics
                | Self::AliyunCmsMetrics
                | Self::TencentMonitorMetrics
                | Self::HuaweiCesMetrics
        )
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AwsCloudwatchLogs => "aws_cloudwatch_logs",
            Self::AwsKinesisFirehose => "aws_kinesis_firehose",
            Self::GcpPubsubPush => "gcp_pubsub_push",
            Self::GcpPubsubPull => "gcp_pubsub_pull",
            Self::CloudEvents => "cloud_events",
            Self::CloudflareLogpush => "cloudflare_logpush",
            Self::HerokuDrain => "heroku_drain",
            Self::S3 => "s3",
            Self::Kafka => "kafka",
            Self::Http => "http",
            Self::KafkaSource => "kafka_source",
            Self::S3Source => "s3_source",
            Self::AwsCloudwatchMetrics => "aws_cloudwatch_metrics",
            Self::AzureMonitorMetrics => "azure_monitor_metrics",
            Self::GcpMonitoringMetrics => "gcp_monitoring_metrics",
            Self::AliyunCmsMetrics => "aliyun_cms_metrics",
            Self::TencentMonitorMetrics => "tencent_monitor_metrics",
            Self::HuaweiCesMetrics => "huawei_ces_metrics",
        }
    }
}

impl std::str::FromStr for ConnectorKind {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "aws_cloudwatch_logs" => Ok(Self::AwsCloudwatchLogs),
            "aws_kinesis_firehose" => Ok(Self::AwsKinesisFirehose),
            "gcp_pubsub_push" => Ok(Self::GcpPubsubPush),
            "gcp_pubsub_pull" => Ok(Self::GcpPubsubPull),
            "cloud_events" => Ok(Self::CloudEvents),
            "cloudflare_logpush" => Ok(Self::CloudflareLogpush),
            "heroku_drain" => Ok(Self::HerokuDrain),
            "s3" => Ok(Self::S3),
            "kafka" => Ok(Self::Kafka),
            "http" => Ok(Self::Http),
            "kafka_source" => Ok(Self::KafkaSource),
            "s3_source" => Ok(Self::S3Source),
            "aws_cloudwatch_metrics" => Ok(Self::AwsCloudwatchMetrics),
            "azure_monitor_metrics" => Ok(Self::AzureMonitorMetrics),
            "gcp_monitoring_metrics" => Ok(Self::GcpMonitoringMetrics),
            "aliyun_cms_metrics" => Ok(Self::AliyunCmsMetrics),
            "tencent_monitor_metrics" => Ok(Self::TencentMonitorMetrics),
            "huawei_ces_metrics" => Ok(Self::HuaweiCesMetrics),
            other => Err(Error::invalid(format!("unknown connector kind: {other}"))),
        }
    }
}

/// pull 型 connector：`run_once` 由 ConnectorRunner 周期调拉取事件（intake 由 runner 统一做）；
/// push 型 connector 不实现。
#[async_trait]
pub trait PullConnector: Send + Sync {
    async fn run_once(&self, ctx: &PullContext) -> Result<PullBatch>;

    /// Called once the batch that carried `receipt` is written: a source that has to tell the
    /// other side what it took (a Pub/Sub subscription's acknowledgements) does it here, so what
    /// failed to be written is delivered again.
    async fn acknowledge(&self, _receipt: Value) -> Result<()> {
        Ok(())
    }
}

pub struct PullContext {
    pub org_id: Id,
    pub connector_id: Id,
    pub last_run_at_micros: Option<i64>,
    /// Where the last pull that was written left off; null before the first one.
    pub position: Value,
}

/// What one pull brought.
#[derive(Debug, Default)]
pub struct PullBatch {
    /// Log events, written to the connector's `targetStream`.
    pub events: Vec<RawEvent>,
    /// Metric samples, written to its `metricsStream`.
    pub metrics: Vec<RawEvent>,
    /// Where the next pull starts, kept once `events` are written. `None`: the source keeps its own
    /// place by `last_run_at`.
    pub position: Option<Value>,
    /// There is more to read right now: the next pull need not wait for the next tick.
    pub more: bool,
    /// Something was skipped for good, and why.
    pub warning: Option<String>,
    /// What to hand [`PullConnector::acknowledge`] once the batch is written.
    pub receipt: Option<Value>,
}
