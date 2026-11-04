// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A Kafka topic as a source: each pull reads on from where the last one that was written left off.
//!
//! - The consumer takes its partitions by assignment, not as a member of a consumer group, and keeps
//!   its offsets in the connector's read state: they move only once the events are written, so a
//!   failed write reads the same messages again (at least once), and it asks of the cluster only what
//!   reading a topic takes.
//! - A new connector starts at the end of each partition or at its beginning (`start_from`). A
//!   partition added later starts at its beginning. An offset the brokers have aged out resumes at
//!   the oldest message they still hold.
//! - A message whose value is a JSON object gives the event's fields; anything else is its `message`.

use std::{
    collections::BTreeMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use rdkafka::{
    ClientContext, Message, Offset, TopicPartitionList,
    config::ClientConfig,
    consumer::{BaseConsumer, Consumer, ConsumerContext},
    error::{KafkaError, RDKafkaErrorCode},
    message::BorrowedMessage,
};
use serde_json::{Map, Value, json};

use crate::{
    domain::intake::RawEvent,
    infra::connectors::{
        PullBatch, PullConnector, PullContext,
        config::{self, StartFrom, kafka::KafkaSettings},
        repo::Connector,
    },
    shared::{Error, Result, time::TimestampMicros},
};

/// A pull stops at this many messages or bytes, and the next one goes on from there.
const MAX_MESSAGES: usize = 10_000;
const MAX_BYTES: usize = 32 * 1024 * 1024;

/// How long a pull reads at most.
const READ_WINDOW: Duration = Duration::from_secs(10);

/// A pull ends once nothing has come for this long after a message.
const IDLE: Duration = Duration::from_secs(2);

/// How long a metadata or watermark request waits for the brokers.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub struct KafkaSource {
    settings: std::result::Result<KafkaSettings, String>,
    start_from: StartFrom,
    group: String,
}

impl KafkaSource {
    pub fn new(connector: &Connector) -> Self {
        Self {
            settings: KafkaSettings::parse(&connector.config_json).map_err(|e| e.to_string()),
            start_from: config::start_from(&connector.config_json).unwrap_or(StartFrom::Latest),
            group: format!("molesignal-{}", connector.id),
        }
    }
}

#[async_trait]
impl PullConnector for KafkaSource {
    async fn run_once(&self, ctx: &PullContext) -> Result<PullBatch> {
        let settings = self.settings.clone().map_err(Error::invalid)?;

        let start_from = self.start_from;
        let group = self.group.clone();
        let position = ctx.position.clone();

        // The client blocks, and so may closing one whose brokers did not answer.
        tokio::task::spawn_blocking(move || consume(&settings, start_from, &group, &position))
            .await
            .map_err(|e| Error::internal(format!("kafka consumer: {e}")))?
    }
}

/// Keeps the last error librdkafka reported: a broker that refuses the TLS handshake or the SASL
/// login shows only as nothing arriving.
#[derive(Default)]
struct ReadContext {
    last_error: Mutex<Option<String>>,
}

impl ClientContext for ReadContext {
    fn error(&self, error: KafkaError, reason: &str) {
        if let Ok(mut last) = self.last_error.lock() {
            *last = Some(format!("{error}: {reason}"));
        }
    }
}

impl ConsumerContext for ReadContext {}

fn consume(
    settings: &KafkaSettings,
    start_from: StartFrom,
    group: &str,
    position: &Value,
) -> Result<PullBatch> {
    let mut client = ClientConfig::new();

    for (key, value) in settings.client_properties() {
        client.set(key, value);
    }

    let consumer: BaseConsumer<ReadContext> = client
        .set("group.id", group)
        .set("enable.auto.commit", "false")
        .set("enable.auto.offset.store", "false")
        .set("auto.offset.reset", "earliest")
        .create_with_context(ReadContext::default())
        .map_err(|e| Error::invalid(format!("kafka client: {e}")))?;

    let reported = |error: String| {
        let last = consumer
            .context()
            .last_error
            .lock()
            .ok()
            .and_then(|last| last.clone());

        match last {
            Some(reason) => format!("{error} ({reason})"),
            None => error,
        }
    };

    let topic = settings.topic.as_str();

    let metadata = consumer
        .fetch_metadata(Some(topic), REQUEST_TIMEOUT)
        .map_err(|e| Error::unavailable(format!("kafka metadata: {}", reported(e.to_string()))))?;

    let partitions: Vec<i32> = metadata
        .topics()
        .iter()
        .find(|listed| listed.name() == topic)
        .map(|listed| listed.partitions().iter().map(|p| p.id()).collect())
        .unwrap_or_default();

    if partitions.is_empty() {
        return Err(Error::not_found(format!(
            "kafka topic `{topic}` is not on these brokers"
        )));
    }

    let saved = saved_offsets(position, topic);
    let first_pull = saved.is_empty();
    let mut offsets = BTreeMap::new();
    let mut assignment = TopicPartitionList::new();
    let mut behind = false;

    for partition in partitions {
        let (low, high) = consumer
            .fetch_watermarks(topic, partition, REQUEST_TIMEOUT)
            .map_err(|e| {
                Error::unavailable(format!("kafka watermarks: {}", reported(e.to_string())))
            })?;

        let next = next_offset(
            saved.get(&partition).copied(),
            first_pull,
            start_from,
            low,
            high,
        );

        // An offset past the end (the topic was made again) is for the consumer to reset.
        behind |= next != high;
        offsets.insert(partition, next);

        assignment
            .add_partition_offset(topic, partition, Offset::Offset(next))
            .map_err(|e| Error::internal(format!("kafka assignment: {e}")))?;
    }

    // Nothing new anywhere: there is no message to wait for.
    if !behind {
        return Ok(PullBatch {
            position: Some(position_of(topic, &offsets)),
            ..PullBatch::default()
        });
    }

    consumer
        .assign(&assignment)
        .map_err(|e| Error::unavailable(format!("kafka assign: {e}")))?;

    // Reaching the partitions' leaders takes a moment, however idle the topic: being idle counts
    // from the first message.
    let started = Instant::now();
    let mut last_message = None::<Instant>;
    let mut events = Vec::new();
    let mut bytes = 0;
    let mut trouble = None;

    while events.len() < MAX_MESSAGES
        && bytes < MAX_BYTES
        && started.elapsed() < READ_WINDOW
        && last_message.is_none_or(|at| at.elapsed() < IDLE)
    {
        match consumer.poll(Duration::from_millis(250)) {
            None => {}
            Some(Ok(message)) => {
                bytes += message.payload_len();
                offsets.insert(message.partition(), message.offset() + 1);
                events.push(event_of(&message));
                last_message = Some(Instant::now());
            }
            Some(Err(error)) if is_about_groups(&error) => {}
            Some(Err(error)) => trouble = Some(error.to_string()),
        }
    }

    // Nothing read and something wrong is a failed pull; what was read counts, and the next pull
    // finds out whether the trouble lasts.
    if events.is_empty()
        && let Some(trouble) = trouble
    {
        return Err(Error::unavailable(format!(
            "kafka read: {}",
            reported(trouble)
        )));
    }

    let more = events.len() >= MAX_MESSAGES || bytes >= MAX_BYTES;

    Ok(PullBatch {
        events,
        position: Some(position_of(topic, &offsets)),
        more,
        warning: None,
    })
}

/// Where a partition is read from: on from the last pull; on the first pull, at the end or the
/// beginning as the connector says; a partition added since, at its beginning.
fn next_offset(
    saved: Option<i64>,
    first_pull: bool,
    start_from: StartFrom,
    low: i64,
    high: i64,
) -> i64 {
    match (saved, first_pull, start_from) {
        (Some(offset), ..) => offset,
        (None, true, StartFrom::Latest) => high,
        _ => low,
    }
}

/// Whether an error is about consumer groups, which a consumer by assignment does not use: librdkafka
/// asks for the group's coordinator all the same, and whoever may only read the topic is refused.
fn is_about_groups(error: &KafkaError) -> bool {
    matches!(
        error,
        KafkaError::MessageConsumption(RDKafkaErrorCode::GroupAuthorizationFailed)
    )
}

/// The offsets a read state holds for this topic: another topic's mean nothing here.
fn saved_offsets(position: &Value, topic: &str) -> BTreeMap<i32, i64> {
    if position.get("topic").and_then(Value::as_str) != Some(topic) {
        return BTreeMap::new();
    }

    position
        .get("offsets")
        .and_then(Value::as_object)
        .map(|offsets| {
            offsets
                .iter()
                .filter_map(|(partition, offset)| Some((partition.parse().ok()?, offset.as_i64()?)))
                .collect()
        })
        .unwrap_or_default()
}

fn position_of(topic: &str, offsets: &BTreeMap<i32, i64>) -> Value {
    let offsets: Map<String, Value> = offsets
        .iter()
        .map(|(partition, offset)| (partition.to_string(), json!(offset)))
        .collect();

    json!({ "topic": topic, "offsets": offsets })
}

fn event_of(message: &BorrowedMessage<'_>) -> RawEvent {
    let timestamp = message
        .timestamp()
        .to_millis()
        .map(|millis| TimestampMicros(millis.saturating_mul(1_000)));

    fields_event(
        message.payload().unwrap_or_default(),
        message.key(),
        message.topic(),
        message.partition(),
        message.offset(),
        timestamp,
    )
}

fn fields_event(
    payload: &[u8],
    key: Option<&[u8]>,
    topic: &str,
    partition: i32,
    offset: i64,
    timestamp: Option<TimestampMicros>,
) -> RawEvent {
    let mut fields = match serde_json::from_slice(payload) {
        Ok(Value::Object(fields)) => fields,
        _ => {
            let text = String::from_utf8_lossy(payload).into_owned();
            Map::from_iter([("message".to_string(), Value::String(text))])
        }
    };

    fields.insert("kafka_topic".into(), json!(topic));
    fields.insert("kafka_partition".into(), json!(partition));
    fields.insert("kafka_offset".into(), json!(offset));

    if let Some(key) = key {
        fields.insert(
            "kafka_key".into(),
            Value::String(String::from_utf8_lossy(key).into_owned()),
        );
    }

    fields
        .entry("source")
        .or_insert_with(|| Value::String("kafka".into()));

    RawEvent {
        timestamp: timestamp.unwrap_or_else(TimestampMicros::now),
        fields,
    }
}

#[cfg(test)]
mod tests {
    use rdkafka::{
        mocking::MockCluster,
        producer::{BaseProducer, BaseRecord, DefaultProducerContext, Producer},
        types::{RDKafkaApiKey, RDKafkaRespErr},
    };

    use super::*;
    use crate::shared::ids::Id;

    const TOPIC: &str = "orders";

    /// librdkafka's own mock brokers: the real client and protocol, with no server to run.
    type Brokers = MockCluster<'static, DefaultProducerContext>;

    fn brokers(partitions: i32) -> Brokers {
        let brokers = Brokers::new(1).unwrap();
        brokers.create_topic(TOPIC, partitions, 1).unwrap();
        brokers
    }

    fn settings(brokers: &Brokers) -> KafkaSettings {
        KafkaSettings::parse(&json!({ "brokers": brokers.bootstrap_servers(), "topic": TOPIC }))
            .unwrap()
    }

    fn produce(brokers: &Brokers, partition: i32, values: &[&str]) {
        produce_to(&brokers.bootstrap_servers(), partition, values);
    }

    fn produce_to(servers: &str, partition: i32, values: &[&str]) {
        let producer: BaseProducer = ClientConfig::new()
            .set("bootstrap.servers", servers)
            .create()
            .unwrap();

        for value in values {
            producer
                .send(
                    BaseRecord::<(), str>::to(TOPIC)
                        .partition(partition)
                        .payload(*value),
                )
                .map_err(|(error, _)| error)
                .unwrap();
        }

        producer.flush(Duration::from_secs(10)).unwrap();
    }

    fn pull(brokers: &Brokers, start_from: StartFrom, position: &Value) -> PullBatch {
        consume(&settings(brokers), start_from, "test", position).unwrap()
    }

    fn messages(batch: &PullBatch) -> Vec<String> {
        let mut messages: Vec<String> = batch
            .events
            .iter()
            .map(|event| event.fields["message"].as_str().unwrap().to_owned())
            .collect();

        messages.sort();
        messages
    }

    #[test]
    fn a_pull_goes_on_from_the_last_one_and_finds_only_what_is_new() {
        let brokers = brokers(2);

        produce(&brokers, 0, &["before"]);

        // From now on: what is there stays unread, and the place is kept.
        let first = pull(&brokers, StartFrom::Latest, &Value::Null);

        assert!(first.events.is_empty());

        let position = first.position.unwrap();

        assert_eq!(
            saved_offsets(&position, TOPIC),
            BTreeMap::from([(0, 1), (1, 0)])
        );

        produce(&brokers, 0, &["a", "b"]);
        produce(&brokers, 1, &["c"]);

        let second = pull(&brokers, StartFrom::Latest, &position);

        assert_eq!(messages(&second), ["a", "b", "c"]);
        assert!(!second.more);
        assert_eq!(second.events[0].fields["kafka_topic"], TOPIC);

        let position = second.position.unwrap();

        assert_eq!(
            saved_offsets(&position, TOPIC),
            BTreeMap::from([(0, 3), (1, 1)])
        );

        let third = pull(&brokers, StartFrom::Latest, &position);

        assert!(third.events.is_empty());
        assert_eq!(third.position.unwrap(), position);
    }

    #[test]
    fn from_the_beginning_reads_it_all_and_a_new_partition_starts_at_its_beginning() {
        let brokers = brokers(2);

        produce(&brokers, 0, &["a"]);
        produce(&brokers, 1, &["b"]);

        let all = pull(&brokers, StartFrom::Earliest, &Value::Null);

        assert_eq!(messages(&all), ["a", "b"]);

        // A place that knows only the first partition: the second was added since.
        let position = position_of(TOPIC, &BTreeMap::from([(0, 1)]));
        let added = pull(&brokers, StartFrom::Latest, &position);

        assert_eq!(messages(&added), ["b"]);
    }

    #[test]
    fn an_offset_past_the_end_goes_on_from_the_oldest_message() {
        let brokers = brokers(1);

        produce(&brokers, 0, &["a", "b"]);

        // The topic was made again, or the offsets belong to another life of it.
        let position = position_of(TOPIC, &BTreeMap::from([(0, 100)]));
        let batch = pull(&brokers, StartFrom::Latest, &position);

        assert_eq!(messages(&batch), ["a", "b"]);
        assert_eq!(
            saved_offsets(&batch.position.unwrap(), TOPIC),
            BTreeMap::from([(0, 2)])
        );
    }

    #[test]
    fn the_offsets_of_another_topic_start_it_over() {
        let brokers = brokers(1);

        produce(&brokers, 0, &["a"]);

        let position = position_of("elsewhere", &BTreeMap::from([(0, 0)]));
        let batch = pull(&brokers, StartFrom::Earliest, &position);

        assert_eq!(messages(&batch), ["a"]);
    }

    #[test]
    fn a_pull_with_more_than_it_takes_says_so() {
        let brokers = brokers(1);
        let values: Vec<String> = (0..=MAX_MESSAGES).map(|index| index.to_string()).collect();
        let values: Vec<&str> = values.iter().map(String::as_str).collect();

        produce(&brokers, 0, &values);

        let first = pull(&brokers, StartFrom::Earliest, &Value::Null);

        assert_eq!(first.events.len(), MAX_MESSAGES);
        assert!(first.more);

        let second = pull(&brokers, StartFrom::Earliest, &first.position.unwrap());

        assert_eq!(second.events.len(), 1);
        assert!(!second.more);
    }

    #[test]
    fn a_first_message_that_takes_its_time_is_still_read() {
        let brokers = brokers(1);
        let servers = brokers.bootstrap_servers();

        // An offset past the end: the pull has something to wait for, and nothing has come yet.
        let position = position_of(TOPIC, &BTreeMap::from([(0, 5)]));

        std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(IDLE + Duration::from_millis(500));
                produce_to(&servers, 0, &["a"]);
            });

            let batch = pull(&brokers, StartFrom::Latest, &position);

            assert_eq!(messages(&batch), ["a"]);
        });
    }

    #[test]
    fn being_refused_a_consumer_group_is_no_trouble() {
        let brokers = brokers(1);

        produce(&brokers, 0, &["a"]);
        brokers.request_errors(
            RDKafkaApiKey::FindCoordinator,
            &[RDKafkaRespErr::RD_KAFKA_RESP_ERR_GROUP_AUTHORIZATION_FAILED; 50],
        );

        let batch = pull(&brokers, StartFrom::Earliest, &Value::Null);

        assert_eq!(messages(&batch), ["a"]);
    }

    #[test]
    fn a_broker_that_refuses_the_read_fails_the_pull_and_says_why() {
        let brokers = brokers(1);

        produce(&brokers, 0, &["a"]);
        brokers.request_errors(
            RDKafkaApiKey::Fetch,
            &[RDKafkaRespErr::RD_KAFKA_RESP_ERR_TOPIC_AUTHORIZATION_FAILED; 50],
        );

        let error = consume(
            &settings(&brokers),
            StartFrom::Earliest,
            "test",
            &Value::Null,
        )
        .unwrap_err();

        assert!(error.to_string().contains("kafka read"), "{error}");
    }

    #[tokio::test]
    async fn a_source_of_a_saved_connector_pulls_and_its_connection_test_finds_the_topic() {
        let brokers = brokers(1);

        produce(&brokers, 0, &[r#"{"level":"warn"}"#]);

        let mut connector = Connector {
            id: Id::from_string("k-1"),
            org_id: Id::from_string("org-1"),
            name: "orders".into(),
            kind: "kafka_source".into(),
            config_json: json!({
                "brokers": brokers.bootstrap_servers(),
                "topic": TOPIC,
                "startFrom": "earliest",
            }),
            enabled: true,
            last_run_at: None,
            read_state: Value::Null,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        };

        let source = KafkaSource::new(&connector);

        let batch = source
            .run_once(&PullContext {
                org_id: connector.org_id.clone(),
                connector_id: connector.id.clone(),
                last_run_at_micros: None,
                position: Value::Null,
            })
            .await
            .unwrap();

        assert_eq!(batch.events[0].fields["level"], "warn");
        assert_eq!(batch.events[0].fields["kafka_offset"], 0);

        let check = crate::infra::connectors::dispatcher::kafka::check;

        check(&connector).await.unwrap();

        // Left alone, these brokers make a topic they are asked about.
        brokers
            .topic_error(
                "not-there",
                RDKafkaRespErr::RD_KAFKA_RESP_ERR_UNKNOWN_TOPIC_OR_PART,
            )
            .unwrap();

        connector.config_json["topic"] = json!("not-there");

        let error = check(&connector).await.unwrap_err();

        assert!(error.to_string().contains("not-there"), "{error}");

        let settings = KafkaSettings::parse(&connector.config_json).unwrap();
        let error = consume(&settings, StartFrom::Latest, "test", &Value::Null).unwrap_err();

        assert!(error.to_string().contains("not-there"), "{error}");
    }

    #[test]
    fn a_json_object_gives_the_fields_and_anything_else_the_message() {
        let event = fields_event(
            br#"{"level":"warn","msg":"retry","source":"checkout"}"#,
            Some(b"order-1"),
            "logs",
            2,
            41,
            Some(TimestampMicros(5_000)),
        );

        assert_eq!(event.timestamp, TimestampMicros(5_000));
        assert_eq!(event.fields["level"], "warn");
        assert_eq!(event.fields["source"], "checkout");
        assert_eq!(event.fields["kafka_topic"], "logs");
        assert_eq!(event.fields["kafka_partition"], 2);
        assert_eq!(event.fields["kafka_offset"], 41);
        assert_eq!(event.fields["kafka_key"], "order-1");

        for payload in [&b"plain text"[..], b"[1, 2]", b"42"] {
            let event = fields_event(payload, None, "logs", 0, 0, None);

            assert_eq!(
                event.fields["message"],
                String::from_utf8_lossy(payload).as_ref()
            );
            assert_eq!(event.fields["source"], "kafka");
            assert!(!event.fields.contains_key("kafka_key"));
        }
    }

    #[test]
    fn a_partition_goes_on_from_its_offset_and_a_new_one_starts_at_its_beginning() {
        let next = |saved, first_pull, start_from| next_offset(saved, first_pull, start_from, 3, 9);

        assert_eq!(next(Some(5), false, StartFrom::Latest), 5);
        assert_eq!(next(None, true, StartFrom::Latest), 9);
        assert_eq!(next(None, true, StartFrom::Earliest), 3);
        assert_eq!(next(None, false, StartFrom::Latest), 3);
    }

    #[test]
    fn only_trouble_with_the_group_is_passed_over() {
        let consuming = KafkaError::MessageConsumption;

        assert!(is_about_groups(&consuming(
            RDKafkaErrorCode::GroupAuthorizationFailed
        )));
        assert!(!is_about_groups(&consuming(
            RDKafkaErrorCode::TopicAuthorizationFailed
        )));
    }

    #[test]
    fn offsets_are_read_back_for_their_own_topic_only() {
        let offsets = BTreeMap::from([(0, 12), (3, 7)]);
        let position = position_of("logs", &offsets);

        assert_eq!(saved_offsets(&position, "logs"), offsets);
        assert!(saved_offsets(&position, "other").is_empty());
        assert!(saved_offsets(&Value::Null, "logs").is_empty());
    }

    #[test]
    fn every_way_of_connecting_makes_a_consumer() {
        // librdkafka checks the properties when the client is made, before connecting anywhere.
        for config in [
            json!({ "brokers": "b:9092", "topic": "t" }),
            json!({
                "brokers": "b:9096",
                "topic": "t",
                "tls": true,
                "auth": "sasl_scram_sha_512",
                "saslUsername": "svc",
                "saslPassword": "pw",
            }),
        ] {
            let settings = KafkaSettings::parse(&config).unwrap();
            let mut client = ClientConfig::new();

            for (key, value) in settings.client_properties() {
                client.set(key, value);
            }

            let consumer: std::result::Result<BaseConsumer<ReadContext>, _> = client
                .set("group.id", "molesignal-test")
                .set("enable.auto.commit", "false")
                .set("enable.auto.offset.store", "false")
                .set("auto.offset.reset", "earliest")
                .create_with_context(ReadContext::default());

            assert!(consumer.is_ok(), "{config}");
        }
    }
}
