// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cloud connectors。
//!
//! 来源（ingress）：
//! - [`cloudwatch_logs::CloudWatchLogsConnector`]：每 30s pull AWS CloudWatch Logs
//! - [`sources::kafka`]、[`sources::s3`]：每 30s 消费 Kafka 主题、读 S3 新对象，读取位点存在
//!   `connectors.read_state`，写入成功后才前进
//! - [`kinesis_firehose`]：push 类共享 HTTP receiver（参考 `api/src/http/routes/intake_kinesis.rs`）
//! - [`cloudflare_logpush`]：HTTP `/api/v1/_cloudflare` receiver（Connector trait 内仅暴露 accept_push）
//! - [`heroku_drain`]：HTTP `/api/v1/_heroku` receiver
//!
//! 目标（egress）：S3、Kafka、HTTP，见 [`dispatcher`]。`ConnectorRunner` 在 `connector` role 内调度
//! pull；push connector 由 HTTP layer 直接走。[`check`] 在保存前试连一个 connector。
//!
//! 配置（`config_json`）按 `kind` 各自定义（见 [`config`]），密钥落库时由 [`secrets`] 封存；
//! shared 字段：`{ target_stream, batch_size_max }`。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    domain::intake::RawEvent,
    shared::{Error, Result, ids::Id},
};

pub mod aws;
pub mod check;
pub mod cloudflare_logpush;
pub mod cloudwatch_logs;
pub mod config;
pub mod dispatcher;
pub mod enrich;
pub mod heroku_drain;
pub mod kinesis_firehose;
pub mod repo;
pub mod runner;
pub mod secrets;
pub mod sources;

pub use dispatcher::{ConnectorDispatcher, EgressDispatcher};
pub use repo::{Connector, ConnectorRepository, PgConnectorRepository};
pub use runner::ConnectorRunner;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorKind {
    AwsCloudwatchLogs,
    AwsKinesisFirehose,
    CloudflareLogpush,
    HerokuDrain,
    S3,
    Kafka,
    Http,
    KafkaSource,
    S3Source,
}

impl ConnectorKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AwsCloudwatchLogs => "aws_cloudwatch_logs",
            Self::AwsKinesisFirehose => "aws_kinesis_firehose",
            Self::CloudflareLogpush => "cloudflare_logpush",
            Self::HerokuDrain => "heroku_drain",
            Self::S3 => "s3",
            Self::Kafka => "kafka",
            Self::Http => "http",
            Self::KafkaSource => "kafka_source",
            Self::S3Source => "s3_source",
        }
    }
}

impl std::str::FromStr for ConnectorKind {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "aws_cloudwatch_logs" => Ok(Self::AwsCloudwatchLogs),
            "aws_kinesis_firehose" => Ok(Self::AwsKinesisFirehose),
            "cloudflare_logpush" => Ok(Self::CloudflareLogpush),
            "heroku_drain" => Ok(Self::HerokuDrain),
            "s3" => Ok(Self::S3),
            "kafka" => Ok(Self::Kafka),
            "http" => Ok(Self::Http),
            "kafka_source" => Ok(Self::KafkaSource),
            "s3_source" => Ok(Self::S3Source),
            other => Err(Error::invalid(format!("unknown connector kind: {other}"))),
        }
    }
}

/// pull 型 connector：`run_once` 由 ConnectorRunner 周期调拉取事件（intake 由 runner 统一做）；
/// push 型 connector 不实现。
#[async_trait]
pub trait PullConnector: Send + Sync {
    async fn run_once(&self, ctx: &PullContext) -> Result<PullBatch>;
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
    pub events: Vec<RawEvent>,
    /// Where the next pull starts, kept once `events` are written. `None`: the source keeps its own
    /// place by `last_run_at`.
    pub position: Option<Value>,
    /// There is more to read right now: the next pull need not wait for the next tick.
    pub more: bool,
    /// Something was skipped for good, and why.
    pub warning: Option<String>,
}
