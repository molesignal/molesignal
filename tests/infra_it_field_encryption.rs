// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 字段级静态加密端到端（per-org DEK）：ingester（RecordBuilder + org DEK）把标 `encrypted`
//! 的字段加密落 parquet（载荷 `kid:<key_id>:v<n>:...`）；查询端 `SELECT col` 拿密文、
//! `SELECT decrypt(col)` 经 FieldKeyService 预载 org DEK 还原明文。
//!
//! 无 docker：in-mem ParquetFileMetaRepository + in-mem CipherKeyRepository + LocalFileSystem。

use std::sync::Arc;

use async_trait::async_trait;
use molesignal::{
    domain::{
        ingestion::RawEvent,
        query::{QueryEngine, QueryLanguage, QueryRequest, StreamHint},
        storage::{ParquetFileMeta, ParquetFileMetaRepository},
        stream::{FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamType},
    },
    infra::{
        cipher::{CipherKey, CipherKeyRepository, FieldKeyService},
        ingester::RecordBuilder,
        search::datafusion_engine::DataFusionEngine,
        storage::parquet::writer::ParquetWriter,
    },
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};
use object_store::{ObjectStore, local::LocalFileSystem};
use parking_lot::Mutex;
use serde_json::json;

// ---- in-mem ParquetFileMetaRepository ----
struct MemParquetFileMetaRepo {
    files: Mutex<Vec<ParquetFileMeta>>,
}
impl MemParquetFileMetaRepo {
    fn new() -> Self {
        Self {
            files: Mutex::new(Vec::new()),
        }
    }
}
#[async_trait]
impl ParquetFileMetaRepository for MemParquetFileMetaRepo {
    async fn insert(&self, file: ParquetFileMeta) -> Result<()> {
        self.files.lock().push(file);
        Ok(())
    }
    async fn find(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        time_range: TimeRange,
    ) -> Result<Vec<ParquetFileMeta>> {
        Ok(self
            .files
            .lock()
            .iter()
            .filter(|f| {
                &f.org_id == org_id
                    && f.stream == stream
                    && f.stream_type == stream_type
                    && !f.deleted
                    && f.time_range.end.0 >= time_range.start.0
                    && f.time_range.start.0 <= time_range.end.0
            })
            .cloned()
            .collect())
    }
    async fn replace(&self, _merged_ids: &[Id], _new_files: Vec<ParquetFileMeta>) -> Result<()> {
        unimplemented!()
    }

    async fn mark_deleted(&self, _ids: &[Id]) -> Result<usize> {
        unimplemented!()
    }
}

// ---- in-mem CipherKeyRepository（raw key 直存，仅测试）----
#[derive(Default)]
struct MemCipherKeys {
    rows: Mutex<Vec<CipherKey>>,
}
#[async_trait]
impl CipherKeyRepository for MemCipherKeys {
    async fn create(&self, org_id: &Id, name: &str, raw_key: &[u8]) -> Result<CipherKey> {
        let mut rows = self.rows.lock();
        if rows.iter().any(|r| &r.org_id == org_id && r.name == name) {
            return Err(Error::invalid("duplicate"));
        }
        let k = CipherKey {
            id: Id::new(),
            org_id: org_id.clone(),
            name: name.to_string(),
            alg: "aes-256-gcm".into(),
            version: 1,
            raw_key: raw_key.to_vec(),
            created_at: TimestampMicros(0),
            rotated_at: None,
        };
        rows.push(k.clone());
        Ok(k)
    }
    async fn rotate(&self, org_id: &Id, name: &str, raw_key: &[u8]) -> Result<CipherKey> {
        let mut rows = self.rows.lock();
        let next = rows
            .iter()
            .filter(|r| &r.org_id == org_id && r.name == name)
            .map(|r| r.version)
            .max()
            .unwrap_or(0)
            + 1;
        let k = CipherKey {
            id: Id::new(),
            org_id: org_id.clone(),
            name: name.to_string(),
            alg: "aes-256-gcm".into(),
            version: next,
            raw_key: raw_key.to_vec(),
            created_at: TimestampMicros(0),
            rotated_at: Some(TimestampMicros(0)),
        };
        rows.push(k.clone());
        Ok(k)
    }
    async fn get_latest(&self, org_id: &Id, name: &str) -> Result<CipherKey> {
        self.rows
            .lock()
            .iter()
            .filter(|r| &r.org_id == org_id && r.name == name)
            .max_by_key(|r| r.version)
            .cloned()
            .ok_or_else(|| Error::not_found("key"))
    }
    async fn get_by_id_version(
        &self,
        org_id: &Id,
        key_id: &str,
        version: i32,
    ) -> Result<CipherKey> {
        self.rows
            .lock()
            .iter()
            .find(|r| &r.org_id == org_id && r.id.0 == key_id && r.version == version)
            .cloned()
            .ok_or_else(|| Error::not_found("key"))
    }
    async fn list(&self, org_id: &Id) -> Result<Vec<CipherKey>> {
        let mut v: Vec<CipherKey> = self
            .rows
            .lock()
            .iter()
            .filter(|r| &r.org_id == org_id)
            .cloned()
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name).then(b.version.cmp(&a.version)));
        Ok(v)
    }
    async fn delete(&self, org_id: &Id, name: &str) -> Result<()> {
        self.rows
            .lock()
            .retain(|r| !(&r.org_id == org_id && r.name == name));
        Ok(())
    }
}

/// `users` stream：`email` 加密、`name` 明文。
fn users_stream(org: &Id) -> StreamDefinition {
    StreamDefinition {
        id: Id::new(),
        org_id: org.clone(),
        name: "users".into(),
        stream_type: StreamType::Logs,
        schema: Schema {
            fields: vec![
                FieldDef {
                    name: "email".into(),
                    data_type: FieldType::Utf8,
                    nullable: true,
                    indexed: false,
                    encrypted: true,
                    exact: false,
                },
                FieldDef {
                    name: "name".into(),
                    data_type: FieldType::Utf8,
                    nullable: false,
                    indexed: false,
                    encrypted: false,
                    exact: false,
                },
            ],
        },
        retention: Some(Retention { days: 30 }),
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
}

fn req(sql: &str, org: &Id) -> QueryRequest {
    QueryRequest {
        org_id: org.clone(),
        language: QueryLanguage::Sql,
        statement: sql.into(),
        time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(10_000_000)),
        stream: Some(StreamHint {
            name: "users".into(),
            stream_type: StreamType::Logs,
        }),
        limit: None,
        federation_clusters: Vec::new(),
    }
}

#[tokio::test]
async fn encrypted_field_round_trips_through_ingest_and_query() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let org = Id::from_string("org-enc");
    let stream = users_stream(&org);

    let key_repo: Arc<dyn CipherKeyRepository> = Arc::new(MemCipherKeys::default());
    let field_keys = Arc::new(FieldKeyService::new(key_repo.clone()));

    // ingest：解析 org DEK（首次自动 provision）→ 注入 RecordBuilder → 加密 email。
    let dek = field_keys.current(&org).await.expect("provision DEK");
    let mut rb = RecordBuilder::new(&stream);
    rb.set_field_key(dek);
    for (email, name) in [("alice@example.com", "alice"), ("bob@example.com", "bob")] {
        let mut fields = serde_json::Map::new();
        fields.insert("email".into(), json!(email));
        fields.insert("name".into(), json!(name));
        rb.push(
            &RawEvent {
                timestamp: TimestampMicros(1_000_000),
                fields,
            },
            1,
        )
        .unwrap();
    }
    let (batch, _) = rb.finish_and_clear().unwrap();
    let writer = ParquetWriter::new(store.clone());
    let meta = writer.flush(&stream, batch).await.unwrap();
    let file_repo = Arc::new(MemParquetFileMetaRepo::new());
    file_repo.insert(meta).await.unwrap();

    let engine =
        DataFusionEngine::new(file_repo.clone(), store.clone()).with_field_keys(field_keys.clone());

    // 直读 email → 密文（kid: 载荷，at-rest 加密）。
    let raw = engine
        .execute(req("SELECT email FROM users ORDER BY name", &org))
        .await
        .expect("select email");
    assert_eq!(raw.rows.len(), 2);
    for row in &raw.rows {
        let stored = row[0].as_str().expect("email is string");
        assert!(
            stored.starts_with("kid:"),
            "stored value must be DEK ciphertext at rest, got {stored}"
        );
    }

    // decrypt(email) → 明文（按 name 排序：alice, bob）。
    let dec = engine
        .execute(req(
            "SELECT decrypt(email) AS email, name FROM users ORDER BY name",
            &org,
        ))
        .await
        .expect("select decrypt(email)");
    assert_eq!(dec.columns, vec!["email", "name"]);
    assert_eq!(dec.rows.len(), 2);
    assert_eq!(dec.rows[0][0], "alice@example.com");
    assert_eq!(dec.rows[0][1], "alice");
    assert_eq!(dec.rows[1][0], "bob@example.com");
    assert_eq!(dec.rows[1][1], "bob");

    // decrypt 套在明文列上是安全透传（非 kid: 前缀 → 原样返回）。
    let passthrough = engine
        .execute(req(
            "SELECT decrypt(name) AS name FROM users ORDER BY name",
            &org,
        ))
        .await
        .expect("decrypt on plaintext col");
    assert_eq!(passthrough.rows[0][0], "alice");
    assert_eq!(passthrough.rows[1][0], "bob");
}
