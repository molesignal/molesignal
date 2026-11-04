// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Single sign-on states (`one_time_tokens` of the `sso_login` and `sso_handoff` kinds) on a real
//! Postgres. Two stores on one database stand for two nodes: they share the
//! database and the root key, and nothing else.
//!
//! Runs on the scratch database named by `MS_IT_SSO_STATES_DSN`, or in a Postgres container with
//! `MS_RUN_IT=1`. It empties those rows and applies the migrations, so never point it at a
//! database anyone uses.

use std::sync::Arc;

use base64::Engine as _;
use molesignal::{
    config::MetaDatabaseSettings,
    infra::{
        cipher::CipherRootKey,
        persistence::MetaStore,
        sso::{SsoHandoff, SsoStateEntry, SsoStateStore, state_store::MAX_ENTRIES},
    },
    shared::Error,
};
use serde_json::json;
use sqlx::{PgPool, Row};
use testcontainers::{ContainerAsync, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres as PgImage;

async fn database() -> Option<(PgPool, Option<ContainerAsync<PgImage>>)> {
    let (dsn, container) = match std::env::var("MS_IT_SSO_STATES_DSN") {
        Ok(dsn) => (dsn, None),
        Err(_) if std::env::var("MS_RUN_IT").as_deref() == Ok("1") => {
            let container = PgImage::default()
                .start()
                .await
                .expect("start postgres container");
            let port = container.get_host_port_ipv4(5432).await.expect("pg port");
            let host = container.get_host().await.expect("pg host");
            let dsn = format!("postgres://postgres:postgres@{host}:{port}/postgres");

            (dsn, Some(container))
        }
        Err(_) => return None,
    };

    let store = MetaStore::connect(&MetaDatabaseSettings {
        dsn,
        min_connections: 2,
        max_connections: 16,
    })
    .await
    .expect("connect and migrate");

    Some((store.pool, container))
}

fn cipher(byte: u8) -> CipherRootKey {
    let key = base64::engine::general_purpose::STANDARD.encode([byte; 32]);

    CipherRootKey::from_base64(&key).expect("a 32-byte key")
}

fn entry(next: &str) -> SsoStateEntry {
    SsoStateEntry {
        provider_id: "provider".into(),
        nonce: "nonce".into(),
        next: Some(next.into()),
        test_id: None,
    }
}

fn session() -> SsoHandoff {
    SsoHandoff {
        body: Ok(json!({ "login": { "userId": "u1", "email": "alice@example.com" } })),
        next: Some("/logs".into()),
    }
}

async fn rows(pool: &PgPool) -> i64 {
    sqlx::query(
        "SELECT COUNT(*)::BIGINT AS n FROM one_time_tokens
         WHERE kind IN ('sso_login', 'sso_handoff')",
    )
    .fetch_one(pool)
    .await
    .expect("count")
    .get("n")
}

/// Fill the table with entries nobody can take, as many requests would.
async fn fill(pool: &PgPool, count: i64) {
    sqlx::query(
        "INSERT INTO one_time_tokens
            (token_hash, kind, secret_nonce, secret_ciphertext, created_at, expires_at)
         SELECT 'filler-' || gen_random_uuid()::TEXT, 'sso_login', '\\x00'::BYTEA, '\\x00'::BYTEA,
                now(), TIMESTAMPTZ '9999-01-01'
         FROM generate_series(1, $1)",
    )
    .bind(count)
    .execute(pool)
    .await
    .expect("fill");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sso_states_on_postgres() {
    let Some((pool, _container)) = database().await else {
        eprintln!("skipped (set MS_IT_SSO_STATES_DSN or MS_RUN_IT=1 to enable)");
        return;
    };

    sqlx::query("DELETE FROM one_time_tokens WHERE kind IN ('sso_login', 'sso_handoff')")
        .execute(&pool)
        .await
        .expect("empty the table");

    let node_a = Arc::new(SsoStateStore::new(pool.clone(), cipher(1), 600));
    let node_b = Arc::new(SsoStateStore::new(pool.clone(), cipher(1), 600));

    an_entry_written_on_one_node_is_taken_once_on_the_other(&node_a, &node_b).await;
    a_handoff_crosses_nodes_with_its_failure_intact(&node_a, &node_b).await;
    the_two_kinds_do_not_answer_for_each_other(&node_a, &node_b).await;
    only_a_digest_and_ciphertext_reach_the_database(&pool, &node_a).await;
    concurrent_takes_have_one_winner(&node_a, &node_b).await;
    expired_entries_are_refused_and_swept(&pool, &node_a).await;
    a_wrong_root_key_is_an_error_not_an_empty_answer(&pool, &node_a).await;
    the_oldest_entries_go_at_the_limit(&pool, &node_a, &node_b).await;
}

async fn an_entry_written_on_one_node_is_taken_once_on_the_other(
    node_a: &SsoStateStore,
    node_b: &SsoStateStore,
) {
    node_a.put("state-1", &entry("/one")).await.expect("put");

    assert_eq!(
        node_b.take("state-1").await.expect("take"),
        Some(entry("/one"))
    );
    assert_eq!(node_b.take("state-1").await.expect("take"), None);
    assert_eq!(node_a.take("state-1").await.expect("take"), None);
    assert_eq!(node_a.take("never-issued").await.expect("take"), None);
}

async fn a_handoff_crosses_nodes_with_its_failure_intact(
    node_a: &SsoStateStore,
    node_b: &SsoStateStore,
) {
    node_a
        .put_handoff("code-1", session())
        .await
        .expect("put handoff");

    let handoff = node_b
        .take_handoff("code-1")
        .await
        .expect("take handoff")
        .expect("the handoff is there");

    assert_eq!(handoff.next.as_deref(), Some("/logs"));
    assert_eq!(handoff.body.expect("a session")["login"]["userId"], "u1");
    assert!(
        node_a
            .take_handoff("code-1")
            .await
            .expect("take handoff")
            .is_none()
    );

    node_b
        .put_handoff(
            "code-2",
            SsoHandoff {
                body: Err(Error::forbidden("provider Acme is disabled")),
                next: None,
            },
        )
        .await
        .expect("put handoff");

    let failure = node_a
        .take_handoff("code-2")
        .await
        .expect("take handoff")
        .expect("the handoff is there")
        .body
        .expect_err("a failure");

    assert_eq!(failure.http_status_code(), 403);
    assert_eq!(failure.to_string(), "forbidden: provider Acme is disabled");
}

/// The `state` of a login someone started must not be spendable as a handoff code, nor use it up.
async fn the_two_kinds_do_not_answer_for_each_other(
    node_a: &SsoStateStore,
    node_b: &SsoStateStore,
) {
    node_a.put("state-2", &entry("/two")).await.expect("put");
    node_a
        .put_handoff("code-3", session())
        .await
        .expect("put handoff");

    assert!(
        node_b
            .take_handoff("state-2")
            .await
            .expect("take handoff")
            .is_none()
    );
    assert_eq!(node_b.take("code-3").await.expect("take"), None);

    assert!(node_b.take("state-2").await.expect("take").is_some());
    assert!(
        node_b
            .take_handoff("code-3")
            .await
            .expect("take handoff")
            .is_some()
    );
}

async fn only_a_digest_and_ciphertext_reach_the_database(pool: &PgPool, node_a: &SsoStateStore) {
    node_a
        .put_handoff("code-in-the-clear", session())
        .await
        .expect("put handoff");

    let row = sqlx::query(
        "SELECT token_hash, kind, secret_nonce, secret_ciphertext
         FROM one_time_tokens WHERE token_hash = encode(sha256(convert_to($1, 'UTF8')), 'hex')",
    )
    .bind("code-in-the-clear")
    .fetch_one(pool)
    .await
    .expect("the row is keyed by the digest of the code");

    let ciphertext: Vec<u8> = row.get("secret_ciphertext");
    let nonce: Vec<u8> = row.get("secret_nonce");

    assert_eq!(row.get::<String, _>("kind"), "sso_handoff");
    assert_eq!(nonce.len(), 12);

    for plain in ["alice@example.com", "login", "/logs"] {
        assert!(
            !ciphertext
                .windows(plain.len())
                .any(|window| window == plain.as_bytes()),
            "{plain} is readable in the row"
        );
    }

    assert!(
        node_a
            .take_handoff("code-in-the-clear")
            .await
            .expect("take handoff")
            .is_some()
    );
}

async fn concurrent_takes_have_one_winner(
    node_a: &Arc<SsoStateStore>,
    node_b: &Arc<SsoStateStore>,
) {
    for round in 0..20 {
        let code = format!("raced-{round}");

        node_a
            .put_handoff(&code, session())
            .await
            .expect("put handoff");

        let takers = (0..16).map(|taker| {
            let node = if taker % 2 == 0 { node_a } else { node_b }.clone();
            let code = code.clone();

            tokio::spawn(async move { node.take_handoff(&code).await.expect("take handoff") })
        });

        let mut winners = 0;

        for taker in takers.collect::<Vec<_>>() {
            if taker.await.expect("taker").is_some() {
                winners += 1;
            }
        }

        assert_eq!(winners, 1, "round {round}");
    }
}

async fn expired_entries_are_refused_and_swept(pool: &PgPool, node_a: &SsoStateStore) {
    let expired_at_once = SsoStateStore::new(pool.clone(), cipher(1), 0);

    // Taken too late: nothing comes back, and the row is gone.
    expired_at_once
        .put("late", &entry("/gone"))
        .await
        .expect("put");

    assert_eq!(expired_at_once.take("late").await.expect("take"), None);
    assert_eq!(rows(pool).await, 0);

    // Never taken: the next entry stored removes it.
    expired_at_once
        .put("forgotten", &entry("/gone"))
        .await
        .expect("put");

    assert_eq!(rows(pool).await, 1);

    node_a.put("fresh", &entry("/fresh")).await.expect("put");

    assert_eq!(rows(pool).await, 1);
    assert!(node_a.take("fresh").await.expect("take").is_some());
    assert_eq!(rows(pool).await, 0);
}

async fn a_wrong_root_key_is_an_error_not_an_empty_answer(pool: &PgPool, node_a: &SsoStateStore) {
    let other_key = SsoStateStore::new(pool.clone(), cipher(2), 600);

    node_a.put("sealed", &entry("/sealed")).await.expect("put");

    let error = other_key
        .take("sealed")
        .await
        .expect_err("cannot be opened");

    assert_eq!(error.http_status_code(), 500);
}

async fn the_oldest_entries_go_at_the_limit(
    pool: &PgPool,
    node_a: &SsoStateStore,
    node_b: &SsoStateStore,
) {
    node_a.put("oldest", &entry("/oldest")).await.expect("put");
    node_a.put("second", &entry("/second")).await.expect("put");

    fill(pool, MAX_ENTRIES - 2).await;
    assert_eq!(rows(pool).await, MAX_ENTRIES);

    // A new sign-in is never turned away. It takes the place of the oldest.
    node_b.put("newest", &entry("/newest")).await.expect("put");

    assert_eq!(rows(pool).await, MAX_ENTRIES);
    assert_eq!(node_a.take("oldest").await.expect("take"), None);
    assert_eq!(rows(pool).await, MAX_ENTRIES);

    assert!(node_a.take("second").await.expect("take").is_some());
    assert!(node_b.take("newest").await.expect("take").is_some());

    // Nodes writing at once can overshoot the limit by what they have in flight. The next entries
    // bring it back, a batch at a time.
    fill(pool, 150).await;
    assert_eq!(rows(pool).await, MAX_ENTRIES - 2 + 150);

    node_a.put("after-1", &entry("/a")).await.expect("put");
    node_b.put("after-2", &entry("/b")).await.expect("put");

    assert!(rows(pool).await <= MAX_ENTRIES);
}
