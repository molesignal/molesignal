// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! An S3 prefix as a source: each pull lists it and reads the objects it has not read, oldest first.
//!
//! - The read state keeps how far the objects' modification times have got and the keys read close
//!   to that moment: S3 gives those times to the second, and an object can show up a little after
//!   others of the same moment. Listing cost grows with what the prefix holds, so give a source a
//!   prefix of its own.
//! - An object is lines: a line that is a JSON object gives an event's fields, one holding `Records`
//!   (CloudTrail and the like) gives an event per record, and anything else is an event's `message`.
//!   `.gz` and `.zst` objects are decompressed.
//! - An object that cannot be fetched stops the pull there, to be tried again. One that cannot be
//!   read (corrupt, too large) is skipped and said in the connector's read state: trying it again
//!   would hold back everything after it for ever.
//! - A pull holds a whole object's events in memory before they are written, so what one object may
//!   hold is limited.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    time::Duration,
};

use async_trait::async_trait;
use flate2::read::GzDecoder;
use futures::StreamExt;
use object_store::{ClientOptions, ObjectStore, ObjectStoreExt, RetryConfig, path::Path};
use serde_json::{Map, Value, json};

use crate::{
    domain::intake::RawEvent,
    infra::connectors::{
        PullBatch, PullConnector, PullContext,
        aws::CredentialSource,
        config::{self, StartFrom},
        dispatcher::s3::{builder, normalize_prefix},
        repo::Connector,
    },
    shared::{Error, Result, time::TimestampMicros},
};

/// How many objects one pull reads at most; the next one goes on from there.
const MAX_OBJECTS: usize = 100;

/// A pull stops adding objects at this many events.
const MAX_EVENTS: usize = 50_000;

/// An object larger than this, stored or decompressed, is skipped.
const MAX_STORED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_DECOMPRESSED_BYTES: u64 = 512 * 1024 * 1024;

/// An object past either of these is skipped too.
const OBJECT_LIMITS: ObjectLimits = ObjectLimits {
    events: 1_000_000,
    line_bytes: 64 * 1024 * 1024,
};

/// How far behind the latest modification time keys are remembered, for objects that show up late.
const LATE_MICROS: i64 = 5 * 60 * 1_000_000;

/// The most keys a read state keeps.
const MAX_SEEN_KEYS: usize = 20_000;

/// How long a request to the bucket may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

pub struct S3Source {
    connector: Connector,
}

impl S3Source {
    pub fn new(connector: &Connector) -> Self {
        Self {
            connector: connector.clone(),
        }
    }

    fn store(&self, source: CredentialSource, timeout: Duration) -> Result<impl ObjectStore> {
        builder(&self.connector, source)?
            .with_retry(RetryConfig {
                max_retries: 1,
                retry_timeout: timeout,
                ..RetryConfig::default()
            })
            .with_client_options(
                ClientOptions::new()
                    .with_timeout(timeout)
                    .with_connect_timeout(Duration::from_secs(10))
                    .with_allow_http(true),
            )
            .build()
            .map_err(|e| Error::invalid(format!("s3: {e}")))
    }

    fn bucket(&self) -> &str {
        config::text(&self.connector.config_json, "bucket")
    }

    fn prefix(&self) -> String {
        normalize_prefix(config::text(&self.connector.config_json, "prefix"))
    }
}

struct ObjectLimits {
    events: usize,
    line_bytes: usize,
}

/// Where a source has read to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Seen {
    /// The latest modification time read, in microseconds.
    since: i64,
    /// The keys read within [`LATE_MICROS`] of it, with their modification times.
    keys: BTreeMap<String, i64>,
}

impl Seen {
    /// What a read state holds for this bucket and prefix: another's mean nothing here. `None`
    /// before the first pull.
    fn read(position: &Value, bucket: &str, prefix: &str) -> Option<Self> {
        let matches = position.get("bucket").and_then(Value::as_str) == Some(bucket)
            && position.get("prefix").and_then(Value::as_str) == Some(prefix);

        if !matches {
            return None;
        }

        Some(Self {
            since: position.get("since").and_then(Value::as_i64).unwrap_or(0),
            keys: position
                .get("keys")
                .and_then(Value::as_object)
                .map(|keys| {
                    keys.iter()
                        .filter_map(|(key, at)| Some((key.clone(), at.as_i64()?)))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }

    fn to_value(&self, bucket: &str, prefix: &str) -> Value {
        let keys: Map<String, Value> = self
            .keys
            .iter()
            .map(|(key, at)| (key.clone(), json!(at)))
            .collect();

        json!({ "bucket": bucket, "prefix": prefix, "since": self.since, "keys": keys })
    }

    /// Whether an object of this time is still to be read.
    fn wants(&self, key: &str, modified: i64) -> bool {
        modified >= self.since.saturating_sub(LATE_MICROS) && !self.keys.contains_key(key)
    }

    /// Remembers an object that was read or skipped.
    fn note(&mut self, key: String, modified: i64) {
        self.since = self.since.max(modified);
        self.keys.insert(key, modified);

        let oldest = self.since.saturating_sub(LATE_MICROS);
        self.keys.retain(|_, at| *at >= oldest);

        // Far more objects in a few minutes than a state should hold: the oldest go first.
        while self.keys.len() > MAX_SEEN_KEYS {
            let Some(key) = self
                .keys
                .iter()
                .min_by_key(|(_, at)| **at)
                .map(|(key, _)| key.clone())
            else {
                break;
            };

            self.keys.remove(&key);
        }
    }
}

#[async_trait]
impl PullConnector for S3Source {
    async fn run_once(&self, ctx: &PullContext) -> Result<PullBatch> {
        let (bucket, prefix) = (self.bucket(), self.prefix());
        let start_from = config::start_from(&self.connector.config_json)?;

        let seen = match Seen::read(&ctx.position, bucket, &prefix) {
            Some(seen) => seen,
            // The first pull: from now on starts here, without listing anything.
            None if start_from == StartFrom::Latest => {
                let seen = Seen {
                    since: TimestampMicros::now().0,
                    keys: BTreeMap::new(),
                };

                return Ok(PullBatch {
                    position: Some(seen.to_value(bucket, &prefix)),
                    ..PullBatch::default()
                });
            }
            None => Seen::default(),
        };

        let store = self.store(CredentialSource::Kept, REQUEST_TIMEOUT)?;

        read_new(&store, bucket, &prefix, seen).await
    }
}

/// Reads the objects under `prefix` that `seen` has not, oldest first, as far as one pull goes.
async fn read_new(
    store: &dyn ObjectStore,
    bucket: &str,
    prefix: &str,
    mut seen: Seen,
) -> Result<PullBatch> {
    let listed = listed_prefix(prefix);
    let mut objects = store.list((!prefix.is_empty()).then_some(&listed));

    // The oldest objects still to be read, and no more than a pull reads, however long the list.
    // They keep the path they were listed with: making it again from their name would encode a `~`.
    let mut wanted: BTreeSet<(i64, Path, u64)> = BTreeSet::new();
    let mut found = 0_usize;

    while let Some(object) = objects.next().await {
        let object = object.map_err(|e| Error::unavailable(format!("s3 list: {e}")))?;
        let modified = object.last_modified.timestamp_micros();

        if !seen.wants(object.location.as_ref(), modified) {
            continue;
        }

        found += 1;
        wanted.insert((modified, object.location, object.size));

        if wanted.len() > MAX_OBJECTS {
            wanted.pop_last();
        }
    }

    drop(objects);

    let mut events = Vec::new();
    let mut read = 0_usize;
    let mut skipped = Vec::new();

    for (modified, location, size) in wanted {
        if events.len() >= MAX_EVENTS {
            break;
        }

        let key = location.to_string();

        if size > MAX_STORED_BYTES {
            skipped.push(format!("`{key}` is larger than {MAX_STORED_BYTES} bytes"));
            seen.note(key, modified);
            read += 1;
            continue;
        }

        let body = match store.get(&location).await {
            Ok(result) => result.bytes().await,
            Err(error) => Err(error),
        };

        let body = match body {
            Ok(body) => body,
            // Gone since it was listed: nothing to read.
            Err(object_store::Error::NotFound { .. }) => {
                seen.note(key, modified);
                read += 1;
                continue;
            }
            // What was read so far counts; the next pull starts at this object.
            Err(error) if read > 0 => {
                tracing::warn!(%key, %error, "s3 source stops at an object it cannot fetch");
                break;
            }
            Err(error) => {
                return Err(Error::unavailable(format!("s3 get `{key}`: {error}")));
            }
        };

        // Decompressing and parsing a large object takes a while.
        let read_object = {
            let (bucket, key) = (bucket.to_owned(), key.clone());

            move || {
                let text = object_lines(&key, &body)?;

                events_of(
                    &text,
                    &bucket,
                    &key,
                    TimestampMicros(modified),
                    &OBJECT_LIMITS,
                )
            }
        };

        let object_events = tokio::task::spawn_blocking(read_object)
            .await
            .map_err(|e| Error::internal(format!("s3 source: {e}")))?;

        match object_events {
            Ok(mut object_events) => events.append(&mut object_events),
            Err(reason) => skipped.push(format!("`{key}`: {reason}")),
        }

        seen.note(key, modified);
        read += 1;
    }

    Ok(PullBatch {
        events,
        position: Some(seen.to_value(bucket, prefix)),
        more: found > read,
        warning: (!skipped.is_empty()).then(|| format!("skipped {}", skipped.join("; "))),
    })
}

/// Lists one object under the prefix: it takes the credentials and the `s3:ListBucket` a pull takes,
/// and finds out whether the bucket is there, without reading anything.
pub async fn check(connector: &Connector) -> Result<()> {
    let source = S3Source::new(connector);
    let store = source.store(CredentialSource::Fresh, Duration::from_secs(10))?;
    let prefix = source.prefix();
    let listed = listed_prefix(&prefix);
    let mut objects = store.list((!prefix.is_empty()).then_some(&listed));

    match objects.next().await {
        None | Some(Ok(_)) => Ok(()),
        Some(Err(error)) => Err(Error::unavailable(format!("s3 list: {error}"))),
    }
}

/// The prefix as the bucket has it. What is not a path to the client is at least tried, encoded.
fn listed_prefix(prefix: &str) -> Path {
    Path::parse(prefix).unwrap_or_else(|_| Path::from(prefix))
}

/// An object's text, decompressed by the end of its name.
fn object_lines(key: &str, body: &[u8]) -> std::result::Result<String, String> {
    let mut text = Vec::new();

    let read = if key.ends_with(".gz") {
        GzDecoder::new(body)
            .take(MAX_DECOMPRESSED_BYTES + 1)
            .read_to_end(&mut text)
    } else if key.ends_with(".zst") {
        zstd::Decoder::new(body).and_then(|decoder| {
            decoder
                .take(MAX_DECOMPRESSED_BYTES + 1)
                .read_to_end(&mut text)
        })
    } else {
        text.extend_from_slice(body);
        Ok(text.len())
    };

    read.map_err(|error| format!("it cannot be decompressed: {error}"))?;

    if text.len() as u64 > MAX_DECOMPRESSED_BYTES {
        return Err(format!(
            "it decompresses to more than {MAX_DECOMPRESSED_BYTES} bytes"
        ));
    }

    Ok(String::from_utf8(text)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned()))
}

fn events_of(
    text: &str,
    bucket: &str,
    key: &str,
    modified: TimestampMicros,
    limits: &ObjectLimits,
) -> std::result::Result<Vec<RawEvent>, String> {
    let mut events = Vec::new();

    let event = |mut fields: Map<String, Value>| {
        fields.insert("s3Bucket".into(), json!(bucket));
        fields.insert("s3Key".into(), json!(key));

        fields
            .entry("source")
            .or_insert_with(|| Value::String("s3".into()));

        RawEvent {
            timestamp: modified,
            fields,
        }
    };

    for line in text.lines() {
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        if line.len() > limits.line_bytes {
            return Err(format!(
                "it has a line longer than {} bytes",
                limits.line_bytes
            ));
        }

        match serde_json::from_str::<Value>(line) {
            Ok(Value::Object(mut fields)) => match fields.remove("Records") {
                Some(Value::Array(records)) if fields.is_empty() => {
                    for record in records {
                        events.push(event(match record {
                            Value::Object(fields) => fields,
                            other => Map::from_iter([("message".into(), other)]),
                        }));
                    }
                }
                records => {
                    if let Some(records) = records {
                        fields.insert("Records".into(), records);
                    }

                    events.push(event(fields));
                }
            },
            _ => events.push(event(Map::from_iter([(
                "message".to_string(),
                Value::String(line.to_owned()),
            )]))),
        }

        if events.len() > limits.events {
            return Err(format!("it holds more than {} events", limits.events));
        }
    }

    Ok(events)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use bytes::Bytes;
    use object_store::{PutPayload, memory::InMemory};

    use super::*;

    const MINUTE: i64 = 60_000_000;

    #[test]
    fn a_line_is_an_event_and_a_records_object_is_one_per_record() {
        let text = r#"{"level":"warn","source":"elb"}
plain text

{"Records":[{"eventName":"PutObject"},{"eventName":"GetObject"}]}
{"Records":[1],"other":true}
"#;

        let events = events_of(
            text,
            "b",
            "logs/1.jsonl",
            TimestampMicros(7),
            &OBJECT_LIMITS,
        )
        .unwrap();

        assert_eq!(events.len(), 5);
        assert_eq!(events[0].fields["level"], "warn");
        assert_eq!(events[0].fields["source"], "elb");
        assert_eq!(events[1].fields["message"], "plain text");
        assert_eq!(events[1].fields["source"], "s3");
        assert_eq!(events[2].fields["eventName"], "PutObject");
        assert_eq!(events[3].fields["eventName"], "GetObject");
        assert_eq!(events[4].fields["other"], true);

        for event in &events {
            assert_eq!(event.fields["s3Bucket"], "b");
            assert_eq!(event.fields["s3Key"], "logs/1.jsonl");
            assert_eq!(event.timestamp, TimestampMicros(7));
        }
    }

    #[test]
    fn an_object_past_its_limits_says_so() {
        let limits = ObjectLimits {
            events: 2,
            line_bytes: 8,
        };
        let events = |text| events_of(text, "b", "k", TimestampMicros(7), &limits);

        assert_eq!(events("one\ntwo\n").unwrap().len(), 2);
        assert_eq!(
            events("one\ntwo\nthree\n").unwrap_err(),
            "it holds more than 2 events"
        );
        assert_eq!(
            events(r#"{"Records":[1,2,3]}"#).unwrap_err(),
            "it has a line longer than 8 bytes"
        );

        let records = ObjectLimits {
            events: 2,
            line_bytes: 64,
        };

        assert_eq!(
            events_of(
                r#"{"Records":[1,2,3]}"#,
                "b",
                "k",
                TimestampMicros(7),
                &records
            )
            .unwrap_err(),
            "it holds more than 2 events"
        );
    }

    #[test]
    fn gz_and_zst_objects_are_decompressed_and_a_corrupt_one_says_so() {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(b"one\ntwo\n").unwrap();
        let gz = gz.finish().unwrap();

        assert_eq!(object_lines("a.log.gz", &gz).unwrap(), "one\ntwo\n");

        let zst = zstd::encode_all(&b"three\n"[..], 1).unwrap();

        assert_eq!(object_lines("a.log.zst", &zst).unwrap(), "three\n");
        assert_eq!(object_lines("a.log", b"four").unwrap(), "four");
        assert!(object_lines("a.log.gz", b"not gzip").is_err());
    }

    #[test]
    fn what_was_read_is_remembered_for_as_long_as_it_could_show_up_late() {
        let mut seen = Seen::default();

        assert!(seen.wants("a", 0));

        seen.note("a".into(), 10 * MINUTE);
        seen.note("b".into(), 12 * MINUTE);

        assert!(!seen.wants("a", 10 * MINUTE));
        assert!(!seen.wants("b", 12 * MINUTE));
        // Late, but within the margin.
        assert!(seen.wants("c", 9 * MINUTE));
        // Too old to be new.
        assert!(!seen.wants("d", 6 * MINUTE));

        seen.note("e".into(), 20 * MINUTE);

        assert!(!seen.keys.contains_key("a") && !seen.keys.contains_key("b"));
        assert!(!seen.wants("a", 10 * MINUTE), "older than the margin now");

        let position = seen.to_value("bucket", "logs/");

        assert_eq!(Seen::read(&position, "bucket", "logs/"), Some(seen));
        assert_eq!(Seen::read(&position, "other", "logs/"), None);
        assert_eq!(Seen::read(&position, "bucket", "elsewhere/"), None);
        assert_eq!(Seen::read(&Value::Null, "bucket", "logs/"), None);
    }

    async fn put(store: &InMemory, key: &str, body: &'static [u8]) {
        store
            .put(&Path::from(key), PutPayload::from(Bytes::from_static(body)))
            .await
            .unwrap();
    }

    fn keys(batch: &PullBatch) -> BTreeSet<String> {
        batch
            .events
            .iter()
            .map(|event| event.fields["s3Key"].as_str().unwrap().to_owned())
            .collect()
    }

    #[tokio::test]
    async fn a_pull_reads_each_new_object_once_and_stays_in_its_prefix() {
        let store = InMemory::new();

        put(&store, "logs/a.jsonl", b"{\"n\":1}\n{\"n\":2}\n").await;
        put(&store, "logs/b.jsonl", b"text\n").await;
        put(&store, "other/c.jsonl", b"{\"n\":3}\n").await;

        let first = read_new(&store, "bucket", "logs/", Seen::default())
            .await
            .unwrap();

        assert_eq!(first.events.len(), 3);
        assert_eq!(
            keys(&first),
            BTreeSet::from(["logs/a.jsonl".to_string(), "logs/b.jsonl".to_string()])
        );
        assert!(!first.more);
        assert_eq!(first.events[0].fields["s3Bucket"], "bucket");

        let position = first.position.unwrap();
        let seen = Seen::read(&position, "bucket", "logs/").unwrap();

        // Nothing new: nothing read, and the same place.
        let again = read_new(&store, "bucket", "logs/", seen.clone())
            .await
            .unwrap();

        assert!(again.events.is_empty());
        assert_eq!(again.position.unwrap(), position);

        put(&store, "logs/d.jsonl", b"{\"n\":4}\n").await;

        let next = read_new(&store, "bucket", "logs/", seen).await.unwrap();

        assert_eq!(keys(&next), BTreeSet::from(["logs/d.jsonl".to_string()]));
    }

    #[tokio::test]
    async fn an_object_whose_name_has_to_be_encoded_is_read_all_the_same() {
        let store = InMemory::new();

        put(&store, "logs/backup~1.log", b"line\n").await;

        let batch = read_new(&store, "bucket", "logs/", Seen::default())
            .await
            .unwrap();

        assert_eq!(batch.events.len(), 1);
        assert!(batch.warning.is_none());
    }

    #[tokio::test]
    async fn a_corrupt_object_is_skipped_and_said_and_the_rest_go_on() {
        let store = InMemory::new();

        put(&store, "logs/bad.log.gz", b"not gzip").await;
        put(&store, "logs/good.log", b"line\n").await;

        let batch = read_new(&store, "bucket", "logs/", Seen::default())
            .await
            .unwrap();

        assert_eq!(keys(&batch), BTreeSet::from(["logs/good.log".to_string()]));
        assert!(batch.warning.unwrap().contains("logs/bad.log.gz"));

        // It is not tried again.
        let seen = Seen::read(&batch.position.unwrap(), "bucket", "logs/").unwrap();
        let again = read_new(&store, "bucket", "logs/", seen).await.unwrap();

        assert!(again.events.is_empty() && again.warning.is_none());
    }

    #[tokio::test]
    async fn a_pull_stops_at_its_limit_and_says_there_is_more() {
        let store = InMemory::new();

        for index in 0..MAX_OBJECTS + 5 {
            store
                .put(
                    &Path::from(format!("logs/{index:04}.log")),
                    PutPayload::from(Bytes::from_static(b"line\n")),
                )
                .await
                .unwrap();
        }

        let first = read_new(&store, "bucket", "logs/", Seen::default())
            .await
            .unwrap();

        assert_eq!(first.events.len(), MAX_OBJECTS);
        assert!(first.more);

        let seen = Seen::read(&first.position.unwrap(), "bucket", "logs/").unwrap();
        let second = read_new(&store, "bucket", "logs/", seen).await.unwrap();

        assert_eq!(second.events.len(), 5);
        assert!(!second.more);
    }
}
