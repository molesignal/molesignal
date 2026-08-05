// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 持久层 happy-path 冒烟（sqlx + Postgres）。
//!
//! 默认会跳过：testcontainers 需要可用的 docker daemon，
//! 设置 `MS_RUN_IT=1` 才真正跑（避免本地无 docker 时单测红）。
//!
//! 运行方式：
//! ```bash
//! MS_RUN_IT=1 cargo test -p molesignal-infra --test it_persistence -- --nocapture
//! ```

use molesignal::{
    config::MetaStoreSettings,
    domain::iam::{Organization, OrganizationRepository, User, UserRepository},
    infra::persistence::{
        MetaStore,
        repositories::{organizations::PgOrganizationRepository, users::PgUserRepository},
    },
    shared::{ids::Id, time::TimestampMicros},
};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres as PgImage;

fn skip_unless_enabled() -> bool {
    std::env::var("MS_RUN_IT").ok().as_deref() != Some("1")
}

#[tokio::test]
async fn org_and_user_roundtrip() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }

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

    let orgs = PgOrganizationRepository::new(store.pool.clone());
    let users = PgUserRepository::new(store.pool.clone());

    let org_id = Id::new();
    let org = orgs
        .create(Organization {
            id: org_id.clone(),
            name: "Acme".into(),
            slug: "acme".into(),
            system: false,
            disabled: false,
            created_at: TimestampMicros::now(),
        })
        .await
        .expect("create org");
    assert_eq!(orgs.get(&org.id).await.expect("get org").name, "Acme");
    assert_eq!(orgs.list().await.expect("list orgs").len(), 1);
    assert_eq!(
        orgs.get_by_slug("acme").await.expect("get by slug").id,
        org_id
    );
    assert!(
        orgs.set_disabled(&org.id, true)
            .await
            .expect_err("last enabled tenant cannot be disabled")
            .to_string()
            .contains("last enabled tenant")
    );
    let second = orgs
        .create(Organization {
            id: Id::new(),
            name: "Beta".into(),
            slug: "beta".into(),
            system: false,
            disabled: false,
            created_at: TimestampMicros::now(),
        })
        .await
        .expect("create second org");
    assert!(
        orgs.set_disabled(&org.id, true)
            .await
            .expect("disable org")
            .disabled
    );
    assert!(
        !orgs
            .set_disabled(&org.id, false)
            .await
            .expect("enable org")
            .disabled
    );
    assert!(
        !orgs
            .get(&second.id)
            .await
            .expect("second org remains")
            .disabled
    );

    let user_id = Id::new();
    let user = users
        .create(User {
            id: user_id.clone(),
            email: "ops@acme.example".into(),
            display_name: "Ops".into(),
            avatar_url: None,
            bio: String::new(),
            password_hash: "$argon2id$dummy".into(),
            disabled: false,
            status: molesignal::domain::iam::UserStatus::Active,
            created_at: TimestampMicros::now(),
        })
        .await
        .expect("create user");
    let fetched = users
        .get_by_email("ops@acme.example")
        .await
        .expect("get by email");
    assert_eq!(fetched.id, user.id);
}
