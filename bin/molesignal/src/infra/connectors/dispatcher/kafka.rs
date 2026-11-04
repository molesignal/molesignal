// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Kafka egress: each event becomes one message on the topic, its JSON as the payload.
//!
//! librdkafka is built with a statically linked OpenSSL (`ssl-vendored`), so TLS, SCRAM and
//! client certificates work without a system OpenSSL, whose version differs from one target
//! distribution to the next. It finds the system's CA certificates by probing the usual paths.

use std::{sync::Mutex, time::Duration};

use rdkafka::{
    ClientContext,
    config::ClientConfig,
    error::KafkaError,
    producer::{
        BaseProducer, DeliveryResult, FutureProducer, FutureRecord, Producer, ProducerContext,
    },
};
use serde_json::Value;

use crate::{
    infra::connectors::{config::kafka::KafkaSettings, repo::Connector},
    shared::{Error, Result},
};

/// How long a connection test waits for the brokers to answer.
const CHECK_TIMEOUT: Duration = Duration::from_secs(10);

fn client_config(settings: &KafkaSettings) -> ClientConfig {
    let mut client = ClientConfig::new();

    for (key, value) in settings.client_properties() {
        client.set(key, value);
    }

    client
}

fn producer(settings: &KafkaSettings) -> Result<FutureProducer> {
    client_config(settings)
        .create()
        .map_err(|e| Error::internal(format!("kafka producer: {e}")))
}

/// Keeps the last error librdkafka reported. A broker that refuses the TLS handshake or the SASL
/// login makes a metadata request time out; this is what says why.
#[derive(Default)]
struct CheckContext {
    last_error: Mutex<Option<String>>,
}

impl ClientContext for CheckContext {
    fn error(&self, error: KafkaError, reason: &str) {
        if let Ok(mut last) = self.last_error.lock() {
            *last = Some(format!("{error}: {reason}"));
        }
    }
}

impl ProducerContext for CheckContext {
    type DeliveryOpaque = ();

    fn delivery(&self, _: &DeliveryResult<'_>, _: Self::DeliveryOpaque) {}
}

pub async fn dispatch(connector: &Connector, events: &[Value]) -> Result<()> {
    let settings = KafkaSettings::parse(&connector.config_json)?;
    let producer = producer(&settings)?;

    for ev in events {
        let payload = serde_json::to_string(ev).unwrap_or_default();
        let record: FutureRecord<'_, str, String> =
            FutureRecord::to(&settings.topic).payload(&payload);

        producer
            .send(record, Duration::from_secs(5))
            .await
            .map_err(|(e, _)| Error::internal(format!("kafka send: {e}")))?;
    }

    Ok(())
}

/// Asks the brokers for the topic's metadata: that takes the connection, the TLS handshake and
/// the SASL or certificate authentication a delivery takes, and finds out whether the topic is
/// there, without writing a message to it.
pub async fn check(connector: &Connector) -> Result<()> {
    let settings = KafkaSettings::parse(&connector.config_json)?;

    let producer: BaseProducer<CheckContext> = client_config(&settings)
        .create_with_context(CheckContext::default())
        .map_err(|e| Error::invalid(format!("kafka client: {e}")))?;

    let topic = settings.topic.clone();

    // The request blocks, and so may closing a client whose brokers did not answer.
    let found = tokio::task::spawn_blocking(move || {
        let metadata = producer
            .client()
            .fetch_metadata(Some(&topic), CHECK_TIMEOUT)
            .map_err(|e| {
                let reported = producer
                    .context()
                    .last_error
                    .lock()
                    .ok()
                    .and_then(|last| last.clone());

                match reported {
                    Some(reason) => format!("{e} ({reason})"),
                    None => e.to_string(),
                }
            })?;

        let found = metadata
            .topics()
            .iter()
            .find(|listed| listed.name() == topic)
            .map(|listed| listed.error());

        Ok::<_, String>(found)
    })
    .await
    .map_err(|e| Error::internal(format!("kafka metadata: {e}")))?
    .map_err(|reason| Error::unavailable(format!("kafka metadata: {reason}")))?;

    match found {
        Some(None) => Ok(()),
        Some(Some(code)) => Err(topic_error(&settings.topic, code)),
        None => Err(Error::not_found(format!(
            "kafka topic `{}` is not on these brokers",
            settings.topic
        ))),
    }
}

/// The error the brokers gave for the topic, in the words the rest of the checks use.
fn topic_error(topic: &str, code: rdkafka::types::RDKafkaRespErr) -> Error {
    use rdkafka::{error::RDKafkaErrorCode, types::RDKafkaRespErr};

    let detail = RDKafkaErrorCode::from(code);

    match code {
        RDKafkaRespErr::RD_KAFKA_RESP_ERR_UNKNOWN_TOPIC_OR_PART => {
            Error::not_found(format!("kafka topic `{topic}`: {detail}"))
        }
        RDKafkaRespErr::RD_KAFKA_RESP_ERR_TOPIC_AUTHORIZATION_FAILED => {
            Error::forbidden(format!("kafka topic `{topic}`: {detail}"))
        }
        _ => Error::unavailable(format!("kafka topic `{topic}`: {detail}")),
    }
}

#[cfg(test)]
mod tests {
    use rdkafka::types::RDKafkaRespErr;
    use serde_json::json;

    use super::*;

    #[test]
    fn every_way_of_connecting_makes_a_client() {
        // librdkafka checks the properties when the client is made, before connecting anywhere.
        for config in [
            json!({ "brokers": "b:9092", "topic": "t" }),
            json!({ "brokers": "b:9093", "topic": "t", "tls": true }),
            json!({
                "brokers": "b:9096",
                "topic": "t",
                "tls": true,
                "auth": "sasl_scram_sha_512",
                "saslUsername": "svc",
                "saslPassword": "pw",
            }),
            json!({
                "brokers": "b:9092",
                "topic": "t",
                "auth": "sasl_scram_sha_256",
                "saslUsername": "svc",
                "saslPassword": "pw",
            }),
        ] {
            let settings = KafkaSettings::parse(&config).unwrap();
            assert!(producer(&settings).is_ok(), "{config}");
        }
    }

    #[test]
    fn a_topic_error_reads_as_what_it_means() {
        assert!(matches!(
            topic_error("t", RDKafkaRespErr::RD_KAFKA_RESP_ERR_UNKNOWN_TOPIC_OR_PART),
            Error::NotFound(_)
        ));
        assert!(matches!(
            topic_error(
                "t",
                RDKafkaRespErr::RD_KAFKA_RESP_ERR_TOPIC_AUTHORIZATION_FAILED
            ),
            Error::Forbidden(_)
        ));
        assert!(matches!(
            topic_error("t", RDKafkaRespErr::RD_KAFKA_RESP_ERR_LEADER_NOT_AVAILABLE),
            Error::Unavailable(_)
        ));
    }
}
