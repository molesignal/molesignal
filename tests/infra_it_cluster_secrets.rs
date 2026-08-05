// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `cluster_secrets` Postgres 往返 + `cipher_keys:<id>` secret_ref 解析端到端冒烟。
//!
//! 覆盖 REMAINING_WORK B1-P4：
//! - PgClusterSecretRepository put/get_plaintext 往返（root key envelope 加密）
//! - upsert 覆盖语义、unknown ref / deleted ref 返错
//! - 密文落库不等于明文（确实加密了）
//! - resolve_secret_ref("cipher_keys:<id>") 经真 repo 解出明文 token
//!
//! 默认跳过：testcontainers 需 docker daemon，设 `MS_RUN_IT=1` 才真跑。
//!
//! ```bash
//! MS_RUN_IT=1 cargo test -p molesignal-infra --test it_cluster_secrets -- --nocapture
//! ```

use molesignal::{
    config::MetaStoreSettings,
    infra::{
        cipher::CipherRootKey,
        cluster::{ClusterSecretRepository, PgClusterSecretRepository},
        persistence::MetaStore,
        secret::resolve_secret_ref,
    },
    shared::ids::Id,
};
use sqlx::Row;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres as PgImage;

fn skip_unless_enabled() -> bool {
    std::env::var("MS_RUN_IT").ok().as_deref() != Some("1")
}

/// 32 字节全零的 base64（仅测试 KEK）。
const ZERO_B64: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

async fn boot() -> MetaStore {
    let pg = PgImage::default().start().await.expect("start pg");
    let port = pg.get_host_port_ipv4(5432).await.expect("pg port");
    let host = pg.get_host().await.expect("pg host");
    let dsn = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    let store = MetaStore::connect(&MetaStoreSettings {
        backend: "postgres".into(),
        dsn,
        min_connections: 1,
        max_connections: 5,
    })
    .await
    .expect("connect + migrate");
    // Leak the testcontainers handle so PG stays up for the test duration.
    std::mem::forget(pg);
    store
}

#[tokio::test]
async fn cluster_secret_roundtrip_and_resolve() {
    if skip_unless_enabled() {
        eprintln!("skipping it_cluster_secrets (set MS_RUN_IT=1)");
        return;
    }
    let store = boot().await;
    let mk = CipherRootKey::from_base64(ZERO_B64).expect("kek");
    let repo = PgClusterSecretRepository::new(store.pool.clone(), mk);
    let org = Id::from_string("orgA");

    // 1. put → get_plaintext 往返。
    repo.put(&org, "sf-token", "Bearer-secret-XYZ")
        .await
        .expect("put");
    assert_eq!(
        repo.get_plaintext(&org, "sf-token").await.expect("get"),
        "Bearer-secret-XYZ"
    );

    // 2. 密文落库 != 明文（确实加密了）。
    let row =
        sqlx::query("SELECT ciphertext FROM cluster_secrets WHERE org_id = $1 AND ref_id = $2")
            .bind(&org.0)
            .bind("sf-token")
            .fetch_one(&store.pool)
            .await
            .expect("row");
    let ct: Vec<u8> = row.try_get("ciphertext").expect("ct");
    assert_ne!(
        ct,
        b"Bearer-secret-XYZ".to_vec(),
        "ciphertext must be sealed"
    );

    // 3. upsert 覆盖。
    repo.put(&org, "sf-token", "Bearer-rotated-2")
        .await
        .expect("re-put");
    assert_eq!(
        repo.get_plaintext(&org, "sf-token").await.expect("get2"),
        "Bearer-rotated-2"
    );

    // 4. org 隔离 + unknown ref 返错。
    assert!(repo.get_plaintext(&org, "missing").await.is_err());
    let other_org = Id::from_string("orgB");
    assert!(repo.get_plaintext(&other_org, "sf-token").await.is_err());

    // 5. resolve_secret_ref 端到端经真 repo 解出明文。
    let token = resolve_secret_ref("cipher_keys:sf-token", &org, Some(&repo))
        .await
        .expect("resolve");
    assert_eq!(token, "Bearer-rotated-2");

    // 6. delete → 后续解析返错。
    repo.delete(&org, "sf-token").await.expect("delete");
    assert!(
        resolve_secret_ref("cipher_keys:sf-token", &org, Some(&repo))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn wrong_kek_fails_to_decrypt() {
    if skip_unless_enabled() {
        eprintln!("skipping it_cluster_secrets (set MS_RUN_IT=1)");
        return;
    }
    let store = boot().await;
    let org = Id::from_string("orgA");

    // 用 KEK A 写入。
    let mk_a = CipherRootKey::from_base64(ZERO_B64).expect("kek a");
    let repo_a = PgClusterSecretRepository::new(store.pool.clone(), mk_a);
    repo_a.put(&org, "t", "plain").await.expect("put");

    // 用不同 KEK B 读 → GCM tag 校验失败，解密返错（不 panic、不泄密）。
    let mk_b =
        CipherRootKey::from_base64("BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=").expect("kek b");
    let repo_b = PgClusterSecretRepository::new(store.pool.clone(), mk_b);
    assert!(repo_b.get_plaintext(&org, "t").await.is_err());
}
