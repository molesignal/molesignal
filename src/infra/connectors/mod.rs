// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cloud connectors。
//!
//! 统一 `Connector` trait + 4 个 initial impl：
//! - [`cloudwatch_logs::CloudWatchLogsConnector`]：每 30s pull AWS CloudWatch Logs
//! - [`kinesis_firehose`]：push 类共享 HTTP receiver（参考 `api/src/http/routes/ingest_kinesis.rs`）
//! - [`cloudflare_logpush`]：HTTP `/api/v1/_cloudflare` receiver（Connector trait 内仅暴露 accept_push）
//! - [`heroku_drain`]：HTTP `/api/v1/_heroku` receiver
//!
//! `ConnectorRunner` 在 `connector` role 内调度 pull；push connector 由 HTTP layer 直接走。
//!
//! 配置（`config_json`）按 `kind` 各自定义；shared 字段：`{ target_stream, batch_size_max }`。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    domain::ingestion::RawEvent,
    shared::{Error, Result, ids::Id},
};

pub mod cloudflare_logpush;
pub mod cloudwatch_logs;
pub mod dispatcher;
pub mod heroku_drain;
pub mod kinesis_firehose;
pub mod repo;
pub mod runner;
pub mod sigv4;

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
            other => Err(Error::invalid(format!("unknown connector kind: {other}"))),
        }
    }
}

/// pull 型 connector：`run_once` 由 ConnectorRunner 周期调拉取事件（ingest 由 runner 统一做）；
/// push 型 connector 不实现。
#[async_trait]
pub trait PullConnector: Send + Sync {
    async fn run_once(&self, ctx: &PullContext) -> Result<Vec<RawEvent>>;
}

pub struct PullContext {
    pub org_id: Id,
    pub connector_id: Id,
    pub last_run_at_micros: Option<i64>,
}
