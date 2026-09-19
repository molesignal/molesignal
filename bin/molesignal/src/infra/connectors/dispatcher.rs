// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Connector egress：把一批事件投递到外部 sink connector。
//!
//! 与 ingress（pull/push 源）相对：`s3` / `kafka` 是 sink——流水线产出经此出向投递。
//! - S3：批量事件序列化为 JSON Lines，PUT 到 `s3://<bucket>/<prefix><id>.jsonl`（复用 object_store）。
//! - Kafka：逐事件作为 JSON 负载 produce 到 topic（rdkafka）。
//!
//! 真实网络投递；本机无 S3/Kafka 时无法端到端验证，纯逻辑（payload 组装、prefix 规整、
//! kind 路由）由单元测试覆盖。

use async_trait::async_trait;
use serde_json::Value;

use super::{ConnectorKind, repo::Connector};
use crate::shared::{Error, Result, ids::Id};

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
            ConnectorKind::S3 => dispatch_s3(connector, events).await,
            ConnectorKind::Kafka => dispatch_kafka(connector, events).await,
            other => Err(Error::invalid(format!(
                "connector kind `{}` is not an egress sink",
                other.as_str()
            ))),
        }
    }
}

fn cfg_str<'a>(connector: &'a Connector, key: &str) -> &'a str {
    connector
        .config_json
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
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

/// 规整对象 key 前缀：去首部 `/`，非空时确保末尾 `/`。
fn normalize_prefix(prefix: &str) -> String {
    let trimmed = prefix.trim().trim_start_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else if trimmed.ends_with('/') {
        trimmed.to_string()
    } else {
        format!("{trimmed}/")
    }
}

async fn dispatch_s3(connector: &Connector, events: &[Value]) -> Result<()> {
    use object_store::{ObjectStoreExt, PutPayload, aws::AmazonS3Builder, path::Path};

    let bucket = cfg_str(connector, "bucket");
    if bucket.is_empty() {
        return Err(Error::invalid("s3 connector requires `bucket`"));
    }
    let mut builder = AmazonS3Builder::new().with_bucket_name(bucket);
    let region = cfg_str(connector, "region");
    if !region.is_empty() {
        builder = builder.with_region(region);
    }
    let endpoint = cfg_str(connector, "endpoint");
    if !endpoint.is_empty() {
        builder = builder.with_endpoint(endpoint).with_allow_http(true);
    }
    let access_key = cfg_str(connector, "access_key");
    if !access_key.is_empty() {
        builder = builder.with_access_key_id(access_key);
    }
    let secret_key = cfg_str(connector, "secret_key");
    if !secret_key.is_empty() {
        builder = builder.with_secret_access_key(secret_key);
    }
    let store = builder
        .build()
        .map_err(|e| Error::internal(format!("s3 egress build: {e}")))?;

    let prefix = normalize_prefix(cfg_str(connector, "prefix"));
    let key = format!("{prefix}{}.jsonl", Id::new().0);
    let body = to_json_lines(events).into_bytes();
    store
        .put(&Path::from(key), PutPayload::from(body))
        .await
        .map_err(|e| Error::internal(format!("s3 egress put: {e}")))?;
    Ok(())
}

async fn dispatch_kafka(connector: &Connector, events: &[Value]) -> Result<()> {
    use std::time::Duration;

    use rdkafka::{
        config::ClientConfig,
        producer::{FutureProducer, FutureRecord},
    };

    let brokers = cfg_str(connector, "brokers");
    let topic = cfg_str(connector, "topic");
    if brokers.is_empty() || topic.is_empty() {
        return Err(Error::invalid(
            "kafka connector requires `brokers` and `topic`",
        ));
    }
    let mut client = ClientConfig::new();
    client.set("bootstrap.servers", brokers);
    let sasl_user = cfg_str(connector, "sasl_username");
    if !sasl_user.is_empty() {
        client
            .set("security.protocol", "sasl_plaintext")
            .set("sasl.mechanism", "PLAIN")
            .set("sasl.username", sasl_user)
            .set("sasl.password", cfg_str(connector, "sasl_password"));
    }
    let producer: FutureProducer = client
        .create()
        .map_err(|e| Error::internal(format!("kafka producer: {e}")))?;

    for ev in events {
        let payload = serde_json::to_string(ev).unwrap_or_default();
        let record: FutureRecord<'_, str, String> = FutureRecord::to(topic).payload(&payload);
        producer
            .send(record, Duration::from_secs(5))
            .await
            .map_err(|(e, _)| Error::internal(format!("kafka send: {e}")))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_normalization() {
        assert_eq!(normalize_prefix(""), "");
        assert_eq!(normalize_prefix("  "), "");
        assert_eq!(normalize_prefix("logs"), "logs/");
        assert_eq!(normalize_prefix("/logs/"), "logs/");
        assert_eq!(normalize_prefix("a/b"), "a/b/");
    }

    #[test]
    fn json_lines_format() {
        let body = to_json_lines(&[serde_json::json!({ "a": 1 }), serde_json::json!({ "b": 2 })]);
        assert_eq!(body, "{\"a\":1}\n{\"b\":2}\n");
        assert_eq!(to_json_lines(&[]), "");
    }
}
