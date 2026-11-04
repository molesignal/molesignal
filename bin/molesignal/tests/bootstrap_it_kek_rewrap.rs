// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `rewrap_all` on a real Postgres: every listed table holds a row sealed with the old key, the
//! statements run against the real schema, and a failure halfway leaves the database as it was.
//!
//! Runs on the scratch database named by `MS_IT_KEK_REWRAP_DSN`, or in a Postgres container with
//! `MS_RUN_IT=1`. It empties the listed tables and applies the migrations, so never point it at a
//! database anyone uses. Seeding switches foreign keys and triggers off, which takes a superuser.

use std::time::Duration;

use base64::Engine as _;
use molesignal::{
    config::MetaDatabaseSettings,
    domain::status_page::StatusPageAccessRepository,
    infra::{
        cipher::{CipherRootKey, kek_rewrap::rewrap_all},
        persistence::{MetaStore, repositories::status_pages::PgStatusPageRepository},
    },
    shared::ids::Id,
};
use sqlx::{PgPool, Row};
use testcontainers::{ContainerAsync, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres as PgImage;

const ORG: &str = "org-a";
const HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

struct SealedColumns {
    nonce: &'static str,
    ciphertext: &'static str,
    hash: Option<&'static str>,
}

const fn sealed(nonce: &'static str, ciphertext: &'static str) -> SealedColumns {
    SealedColumns {
        nonce,
        ciphertext,
        hash: None,
    }
}

const fn hashed(
    nonce: &'static str,
    ciphertext: &'static str,
    hash: &'static str,
) -> SealedColumns {
    SealedColumns {
        nonce,
        ciphertext,
        hash: Some(hash),
    }
}

/// One row of a listed table: what it needs besides the sealed columns.
struct Seed {
    table: &'static str,
    columns: &'static str,
    values: &'static str,
    sealed: &'static [SealedColumns],
}

const SEEDS: &[Seed] = &[
    Seed {
        table: "cipher_keys",
        columns: "id, org_id, name, created_at",
        values: "'r1', 'org-a', 'key', to_timestamp(1)",
        sealed: &[sealed("nonce", "key_material_enc")],
    },
    Seed {
        table: "sealed_secrets",
        columns: "org_id, owner_kind, owner_id, created_at, updated_at",
        values: "'org-a', 'cluster', 'r1', to_timestamp(1), to_timestamp(1)",
        sealed: &[sealed("nonce", "ciphertext")],
    },
    Seed {
        table: "api_tokens",
        columns: "id, org_id, prefix, secret_hash, name, policy_id, created_at, \
                  token_kind, is_default",
        values: "'r1', 'org-a', 'prefix', 'hash', 'token', 'builtin:data_intake', \
                 to_timestamp(1), 'default_intake', TRUE",
        sealed: &[sealed("plaintext_nonce", "plaintext_sealed")],
    },
    Seed {
        table: "resource_shares",
        columns: "id, organization_id, resource_type, resource_id, share_mode, token_hash, \
                  created_by, created_at",
        values: "'r1', 'org-a', 'dashboard', 'd1', 'public_link', 'hash', 'u1', to_timestamp(1)",
        sealed: &[sealed("token_nonce", "token_ciphertext")],
    },
    Seed {
        table: "notify_connectors",
        columns: "id, organization_id, name, connector_type, created_at, updated_at",
        values: "'r1', 'org-a', 'connector', 'webhook', to_timestamp(1), to_timestamp(1)",
        sealed: &[sealed("config_nonce", "config_ciphertext")],
    },
    Seed {
        table: "connectors",
        columns: "id, org_id, name, kind, config_json, created_at, updated_at",
        values: "'r1', 'org-a', 'connector', 'kafka', '{}', to_timestamp(1), to_timestamp(1)",
        sealed: &[sealed("secrets_nonce", "secrets_ciphertext")],
    },
    Seed {
        table: "alert_source_mailboxes",
        columns: "source_id, org_id, settings, updated_at",
        values: "'r1', 'org-a', '{}', to_timestamp(1)",
        sealed: &[sealed("secret_nonce", "secret_ciphertext")],
    },
    Seed {
        table: "ticket_connectors",
        columns: "id, org_id, name, system, created_at, updated_at",
        values: "'r1', 'org-a', 'connector', 'jira', to_timestamp(1), to_timestamp(1)",
        sealed: &[sealed("secret_nonce", "secret_ciphertext")],
    },
    Seed {
        table: "org_settings",
        columns: "org_id, section, updated_at",
        values: "'org-a', 'incident.response', to_timestamp(1)",
        sealed: &[sealed("secret_nonce", "secret_ciphertext")],
    },
    Seed {
        table: "remediation_actions",
        columns: "id, org_id, name, target, created_at, updated_at",
        values: "'r1', 'org-a', 'action', '{}', to_timestamp(1), to_timestamp(1)",
        sealed: &[sealed("secret_nonce", "secret_ciphertext")],
    },
    Seed {
        table: "kubernetes_clusters",
        columns: "id, org_id, name, api_server, created_at, updated_at",
        values: "'r1', 'org-a', 'cluster', 'https://k8s.example.com', to_timestamp(1), \
                 to_timestamp(1)",
        sealed: &[sealed("token_nonce", "token_ciphertext")],
    },
    Seed {
        table: "synthetic_secret_versions",
        columns: "secret_id, organization_id, version, created_by, created_at",
        values: "'r1', 'org-a', 1, 'u1', to_timestamp(1)",
        sealed: &[sealed("nonce", "ciphertext")],
    },
    Seed {
        table: "synthetic_probe_certificate_authorities",
        columns: "id, certificate_pem, created_at",
        values: "'r1', 'pem', to_timestamp(1)",
        sealed: &[sealed("private_key_nonce", "private_key_ciphertext")],
    },
    Seed {
        table: "synthetic_monitor_revisions",
        columns: "id, organization_id, monitor_id, revision_number, spec, schedule, \
                  timeout_millis, freshness_seconds, location_policy, created_by, \
                  created_at, content_hash",
        values: "'r1', 'org-a', 'm1', 1, '{}', '{}', 1000, 60, '{}', 'u1', to_timestamp(1), \
                 'hash'",
        sealed: &[sealed("client_key_nonce", "client_key_ciphertext")],
    },
    Seed {
        table: "status_page_subscribers",
        columns: "id, org_id, status_page_id, channel, status, token_hash, created_at, \
                  updated_at",
        values: "'r1', 'org-a', 'p1', 'email', 'active', 'hash', to_timestamp(1), to_timestamp(1)",
        sealed: &[
            hashed("target_nonce", "target_ciphertext", "target_hash"),
            sealed("token_nonce", "token_ciphertext"),
        ],
    },
    Seed {
        table: "status_page_access_rules",
        columns: "id, org_id, status_page_id, kind, created_at, updated_at",
        values: "'r1', 'org-a', 'p1', 'email', to_timestamp(1), to_timestamp(1)",
        sealed: &[hashed("value_nonce", "value_ciphertext", "value_hash")],
    },
    Seed {
        table: "status_page_magic_links",
        columns: "id, org_id, status_page_id, access_rule_id, token_hash, origin_host, \
                  expires_at, created_at",
        values: "'r1', 'org-a', 'p1', 'r1', 'hash', 'status.example.com', to_timestamp(2), \
                 to_timestamp(1)",
        sealed: &[hashed("email_nonce", "email_ciphertext", "email_hash")],
    },
    Seed {
        table: "status_page_access_sessions",
        columns: "id, org_id, status_page_id, access_rule_id, session_token_hash, origin_host, \
                  expires_at, last_seen_at, created_at",
        values: "'r1', 'org-a', 'p1', 'r1', 'hash', 'status.example.com', to_timestamp(2), \
                 to_timestamp(1), to_timestamp(1)",
        sealed: &[hashed("email_nonce", "email_ciphertext", "email_hash")],
    },
    Seed {
        table: "users_ext",
        columns: "user_id, totp_created_at",
        values: "'u1', to_timestamp(1)",
        sealed: &[sealed("totp_secret_nonce", "totp_secret_ciphertext")],
    },
    Seed {
        table: "one_time_tokens",
        columns: "token_hash, kind, created_at, expires_at",
        values: "'r1', 'sso_login', to_timestamp(1), to_timestamp(2)",
        sealed: &[sealed("secret_nonce", "secret_ciphertext")],
    },
    Seed {
        table: "signing_secrets",
        columns: "id, kind, is_primary, created_at",
        values: "'r1', 'jwt', TRUE, to_timestamp(1)",
        sealed: &[sealed("secret_nonce", "secret_ciphertext")],
    },
];

async fn database() -> Option<(PgPool, Option<ContainerAsync<PgImage>>)> {
    let (dsn, container) = match std::env::var("MS_IT_KEK_REWRAP_DSN") {
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
        max_connections: 8,
    })
    .await
    .expect("connect and migrate");

    Some((store.pool, container))
}

fn key(byte: u8) -> CipherRootKey {
    let key = base64::engine::general_purpose::STANDARD.encode([byte; 32]);

    CipherRootKey::from_base64(&key).expect("a 32-byte key")
}

/// What a sealed column holds; every column holds something else.
fn plain(table: &str, ciphertext: &str) -> String {
    format!("{table}.{ciphertext}")
}

async fn seed(pool: &PgPool, sealing: &CipherRootKey) {
    let mut transaction = sqlx::begin(pool).await.expect("begin");

    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .expect("foreign keys off");

    for seed in SEEDS {
        sqlx::query(&format!("DELETE FROM {}", seed.table))
            .execute(&mut *transaction)
            .await
            .expect("empty the table");

        let mut columns = vec![seed.columns.to_string()];
        let mut values = vec![seed.values.to_string()];
        let mut blobs = Vec::new();

        for pair in seed.sealed {
            let (nonce, ciphertext) = sealing
                .seal(plain(seed.table, pair.ciphertext).as_bytes())
                .expect("seal");

            columns.push(format!("{}, {}", pair.nonce, pair.ciphertext));
            values.push(format!("${}, ${}", blobs.len() + 1, blobs.len() + 2));
            blobs.push(nonce);
            blobs.push(ciphertext);

            // The value is wrong on purpose: only the new key's hash may come out.
            if let Some(hash) = pair.hash {
                columns.push(hash.to_string());
                values.push(format!("'{HASH}'"));
            }
        }

        let statement = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            seed.table,
            columns.join(", "),
            values.join(", ")
        );

        let mut insert = sqlx::query(&statement);

        for blob in &blobs {
            insert = insert.bind(blob.as_slice());
        }

        insert
            .execute(&mut *transaction)
            .await
            .unwrap_or_else(|error| panic!("seed {}: {error}", seed.table));
    }

    transaction.commit().await.expect("commit");
}

/// Every sealed column of every listed table opens with `current`, and only with it.
async fn assert_sealed_with(pool: &PgPool, current: &CipherRootKey, other: &CipherRootKey) {
    for seed in SEEDS {
        for pair in seed.sealed {
            let row = sqlx::query(&format!(
                "SELECT {}, {} FROM {}",
                pair.nonce, pair.ciphertext, seed.table
            ))
            .fetch_one(pool)
            .await
            .unwrap_or_else(|error| panic!("read {}: {error}", seed.table));

            let nonce: Vec<u8> = row.get(pair.nonce);
            let ciphertext: Vec<u8> = row.get(pair.ciphertext);
            let expected = plain(seed.table, pair.ciphertext);

            assert_eq!(
                current.open(&nonce, &ciphertext).ok().as_deref(),
                Some(expected.as_bytes()),
                "{}.{}",
                seed.table,
                pair.ciphertext
            );
            assert!(
                other.open(&nonce, &ciphertext).is_err(),
                "{}.{}",
                seed.table,
                pair.ciphertext
            );
        }
    }
}

/// Hashes follow the plaintext to the new key, in each table that keeps one.
async fn assert_hashed_with(pool: &PgPool, current: &CipherRootKey) {
    for seed in SEEDS {
        for pair in seed.sealed {
            let Some(hash) = pair.hash else {
                continue;
            };

            let row = sqlx::query(&format!("SELECT {hash} FROM {}", seed.table))
                .fetch_one(pool)
                .await
                .unwrap_or_else(|error| panic!("read {}: {error}", seed.table));

            let stored: String = row.get(hash);
            let value = plain(seed.table, pair.ciphertext);

            assert_eq!(
                stored,
                current.org_hmac_sha256(ORG, value.as_bytes()),
                "{}.{hash}",
                seed.table
            );
        }
    }
}

async fn flip_first_byte(pool: &PgPool, seed: &Seed) {
    let column = seed.sealed[0].ciphertext;

    sqlx::query(&format!(
        "UPDATE {} SET {column} = set_byte({column}, 0, get_byte({column}, 0) # 1)",
        seed.table
    ))
    .execute(pool)
    .await
    .expect("flip a bit");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rewrap_kek_on_postgres() {
    let Some((pool, _container)) = database().await else {
        eprintln!("skipped (set MS_IT_KEK_REWRAP_DSN or MS_RUN_IT=1 to enable)");
        return;
    };

    let (first, second, third, fourth) = (key(1), key(2), key(3), key(4));

    seed(&pool, &first).await;
    assert_sealed_with(&pool, &first, &second).await;

    // A table added to the list without a seed stops here.
    let report = rewrap_all(&pool, &first, &second)
        .await
        .expect("rewrap every table");

    assert_eq!(
        report,
        SEEDS
            .iter()
            .map(|seed| (seed.table.to_string(), 1))
            .collect::<Vec<_>>(),
    );

    assert_sealed_with(&pool, &second, &first).await;
    assert_hashed_with(&pool, &second).await;

    // A visitor is found by the hash of their address, under the new key.
    let status_pages = PgStatusPageRepository::new(pool.clone(), second.clone());
    let visitor = plain("status_page_access_rules", "value_ciphertext");

    let rule = status_pages
        .find_access_rule_for_email(&Id::from_string(ORG), &Id::from_string("p1"), &visitor, "x")
        .await
        .expect("look the visitor up")
        .expect("the rule is still found");

    assert_eq!(rule.value, visitor);

    // A row the command cannot open, in the table it reaches last: every table before it has been
    // rewrapped inside the transaction by then.
    let (last, _) = report.last().expect("a report");

    let damaged = SEEDS
        .iter()
        .find(|seed| seed.table == last)
        .expect("a seed");

    flip_first_byte(&pool, damaged).await;

    let error = rewrap_all(&pool, &second, &third)
        .await
        .expect_err("a row that cannot be opened")
        .to_string();

    assert!(error.contains(last), "{error}");
    assert!(error.contains("rewrap open with old KEK"), "{error}");

    // Nothing changed: repairing the row is all a second attempt needs.
    flip_first_byte(&pool, damaged).await;
    assert_sealed_with(&pool, &second, &third).await;
    assert_hashed_with(&pool, &second).await;

    rewrap_all(&pool, &second, &third)
        .await
        .expect("the attempt after the repair");

    assert_sealed_with(&pool, &third, &second).await;
    assert_hashed_with(&pool, &third).await;

    // Running a finished rotation again says so and leaves the rows alone.
    let error = rewrap_all(&pool, &second, &third)
        .await
        .expect_err("already finished")
        .to_string();

    assert!(error.contains("already sealed with the new KEK"), "{error}");
    assert_sealed_with(&pool, &third, &second).await;

    // A write still open when the command starts is waited for and rewrapped, not missed.
    let (nonce, ciphertext) = third.seal(b"late").expect("seal");
    let mut writer = sqlx::begin(&pool).await.expect("begin");

    sqlx::query(
        "INSERT INTO sealed_secrets
            (org_id, owner_kind, owner_id, ciphertext, nonce, created_at, updated_at)
         VALUES ('org-b', 'cluster', 'late', $1, $2, now(), now())",
    )
    .bind(ciphertext.as_slice())
    .bind(nonce.as_slice())
    .execute(&mut *writer)
    .await
    .expect("a write in progress");

    let running = tokio::spawn({
        let (pool, old, new) = (pool.clone(), third.clone(), fourth.clone());

        async move { rewrap_all(&pool, &old, &new).await }
    });

    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!running.is_finished(), "the command waits for the write");
    writer.commit().await.expect("commit the write");

    let report = running.await.expect("join").expect("rewrap");

    let rows = report
        .iter()
        .find(|(table, _)| table == "sealed_secrets")
        .map(|(_, rows)| *rows);

    assert_eq!(rows, Some(2));

    let row = sqlx::query("SELECT nonce, ciphertext FROM sealed_secrets WHERE owner_id = 'late'")
        .fetch_one(&pool)
        .await
        .expect("the late row");

    let nonce: Vec<u8> = row.get("nonce");
    let ciphertext: Vec<u8> = row.get("ciphertext");
    assert_eq!(fourth.open(&nonce, &ciphertext).expect("open"), b"late");
}
