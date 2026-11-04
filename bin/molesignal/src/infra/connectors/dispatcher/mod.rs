// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Connector egress：把一批事件投递到外部 sink connector。
//!
//! 与 ingress（pull/push 源）相对：`s3` / `kafka` / `http` 是 sink——流水线产出经此出向投递。
//! - [`s3`]：批量事件序列化为 JSON Lines，PUT 到 `s3://<bucket>/<prefix><id>.jsonl`（复用
//!   object_store）。凭据按 `auth`：访问密钥、临时密钥、节点身份、扮演角色、Web Identity（见
//!   [`super::aws`]）。`endpoint` 指向 S3 兼容存储，`virtualHostedStyle` 切换为虚拟主机式寻址。
//! - [`kafka`]：逐事件作为 JSON 负载 produce 到 topic（rdkafka，静态链接 OpenSSL）：明文或 TLS，
//!   SASL PLAIN / SCRAM-SHA-256 / SCRAM-SHA-512 或 TLS 客户端证书。
//! - [`http`]：一批事件作为 NDJSON 或 JSON 数组 POST / PUT 到一个地址，按 `auth` 带 Bearer、
//!   Basic、自定义头或 HMAC 签名。
//!
//! 真实网络投递；本机无 S3/Kafka 时无法端到端验证，纯逻辑（payload 组装、prefix 规整、
//! 客户端参数、签名、kind 路由）由单元测试覆盖。

use async_trait::async_trait;
use serde_json::Value;

use super::{ConnectorKind, aws::CredentialSource, repo::Connector};
use crate::shared::{Error, Result};

pub mod http;
pub mod kafka;
pub mod s3;

#[async_trait]
pub trait ConnectorDispatcher: Send + Sync {
    /// 把 `events` 投递到该 connector。空批次为 no-op。
    async fn dispatch(&self, connector: &Connector, events: &[Value]) -> Result<()>;
}

/// 按 `connector.kind` 路由到具体 egress 实现。
pub struct EgressDispatcher;

#[async_trait]
impl ConnectorDispatcher for EgressDispatcher {
    async fn dispatch(&self, connector: &Connector, events: &[Value]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }

        match connector.kind.parse::<ConnectorKind>()? {
            ConnectorKind::S3 => s3::dispatch(connector, events, CredentialSource::Kept).await,
            ConnectorKind::Kafka => kafka::dispatch(connector, events).await,
            ConnectorKind::Http => http::dispatch(connector, events).await,
            other => Err(Error::invalid(format!(
                "connector kind `{}` is not an egress sink",
                other.as_str()
            ))),
        }
    }
}

/// 批量事件 → JSON Lines（每行一个 JSON 对象）。
fn to_json_lines(events: &[Value]) -> String {
    let mut body = String::new();

    for ev in events {
        if let Ok(line) = serde_json::to_string(ev) {
            body.push_str(&line);
            body.push('\n');
        }
    }

    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_lines_format() {
        let body = to_json_lines(&[serde_json::json!({ "a": 1 }), serde_json::json!({ "b": 2 })]);
        assert_eq!(body, "{\"a\":1}\n{\"b\":2}\n");
        assert_eq!(to_json_lines(&[]), "");
    }

    #[tokio::test]
    async fn a_source_is_not_delivered_to() {
        use crate::shared::{ids::Id, time::TimestampMicros};

        let source = Connector {
            id: Id::new(),
            org_id: Id::new(),
            name: "drain".into(),
            kind: "heroku_drain".into(),
            config_json: serde_json::json!({}),
            enabled: true,
            last_run_at: None,
            read_state: serde_json::Value::Null,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        };

        let error = EgressDispatcher
            .dispatch(&source, &[serde_json::json!({ "a": 1 })])
            .await
            .unwrap_err();

        assert!(error.to_string().contains("not an egress sink"));
    }
}
