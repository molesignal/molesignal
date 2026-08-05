// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Intelligence 持久化层 Postgres 往返。
//!
//! 覆盖：
//! - provider CRUD + 加密 key 往返（密文 != 明文、masked key_last4、rotate、update 不动 key）
//! - prompt 解析顺序（builtin → org → user 默认）+ override 递增 version + builtin 不可变
//! - archive 元数据写入 / 列表 / failed 记录 / retention 删除返回 object_key
//! - chat 软删（normal 列表过滤、get_chat 报错、get_chat_any 仍可解析）
//!
//! 默认跳过：testcontainers 需 docker daemon，设 `MS_RUN_IT=1` 才真跑。
//!
//! ```bash
//! MS_RUN_IT=1 cargo test -p molesignal-infra --test it_ai_anomaly_chat -- --nocapture
//! ```

use molesignal::{
    config::MetaStoreSettings,
    infra::{
        cipher::CipherRootKey,
        persistence::{
            MetaStore,
            repositories::{
                audit_events::{
                    AuditEvent, AuditEventRepository, AuditQuery, PgAuditEventRepository,
                },
                intelligence::{
                    chat_archives::{ChatArchive, ChatArchiveRepository, PgChatArchiveRepository},
                    chats::{Chat, ChatRepository, PgChatRepository},
                    model_providers::{
                        ModelProviderInput, ModelProviderRepository, PgModelProviderRepository,
                    },
                    prompts::{
                        AgentPromptRepository, AgentPromptTemplate, PgAgentPromptRepository,
                    },
                },
            },
        },
    },
    shared::{ids::Id, time::TimestampMicros},
};
use serde_json::json;
use sqlx::Row;

fn skip_unless_enabled() -> bool {
    std::env::var("MS_RUN_IT").ok().as_deref() != Some("1")
}

const ZERO_B64: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

async fn boot() -> MetaStore {
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres as PgImage;
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
    std::mem::forget(pg);
    store
}

fn provider_input(id: &Id, org: &Id) -> ModelProviderInput {
    ModelProviderInput {
        id: id.clone(),
        org_id: org.clone(),
        provider: "openai".into(),
        name: "primary".into(),
        base_url: Some("https://api.openai.com/v1".into()),
        default_model: "gpt-4o".into(),
        enabled: true,
        timeout_ms: 30_000,
        max_tokens: Some(4096),
    }
}

#[tokio::test]
async fn provider_crud_and_encrypted_key_roundtrip() {
    if skip_unless_enabled() {
        eprintln!("skipping it_ai_anomaly_chat (set MS_RUN_IT=1)");
        return;
    }
    let store = boot().await;
    let mk = CipherRootKey::from_base64(ZERO_B64).expect("kek");
    let repo = PgModelProviderRepository::new(store.pool.clone(), mk);
    let org = Id::from_string("orgA");
    let id = Id::from_string("prov1");

    // 1. create with key → masked 元数据，无明文。
    let p = repo
        .create(provider_input(&id, &org), Some("sk-secret-ABCD1234"))
        .await
        .expect("create");
    assert_eq!(p.key_last4.as_deref(), Some("1234"));
    assert!(p.key_set);

    // 2. 密文落库 != 明文。
    let row = sqlx::query(
        "SELECT ciphertext FROM intelligence_model_provider_secrets WHERE provider_id = $1",
    )
    .bind(&id.0)
    .fetch_one(&store.pool)
    .await
    .expect("secret row");
    let ct: Vec<u8> = row.try_get("ciphertext").expect("ct");
    assert_ne!(ct, b"sk-secret-ABCD1234".to_vec());

    // 3. get_plaintext_key 解出明文。
    assert_eq!(
        repo.get_plaintext_key(&org, &id).await.expect("plain"),
        Some("sk-secret-ABCD1234".to_string())
    );

    // 4. update 元数据不动 key。
    let mut upd = provider_input(&id, &org);
    upd.name = "renamed".into();
    upd.enabled = false;
    let p2 = repo.update(upd).await.expect("update");
    assert_eq!(p2.name, "renamed");
    assert!(!p2.enabled);
    assert!(p2.key_set, "update must not clear key");
    assert_eq!(
        repo.get_plaintext_key(&org, &id).await.expect("plain2"),
        Some("sk-secret-ABCD1234".to_string())
    );

    // 5. rotate_key 改 key + last4。
    let p3 = repo
        .rotate_key(&org, &id, "sk-rotated-WXYZ9876")
        .await
        .expect("rotate");
    assert_eq!(p3.key_last4.as_deref(), Some("9876"));
    assert_eq!(
        repo.get_plaintext_key(&org, &id).await.expect("plain3"),
        Some("sk-rotated-WXYZ9876".to_string())
    );

    // 6. org 隔离：别的 org 取不到。
    let other = Id::from_string("orgB");
    assert!(repo.get(&other, &id).await.is_err());

    // 7. delete 清 provider + secret。
    repo.delete(&org, &id).await.expect("delete");
    assert!(repo.get(&org, &id).await.is_err());
    assert_eq!(
        repo.get_plaintext_key(&org, &id).await.expect("post-del"),
        None
    );
}

#[tokio::test]
async fn prompt_resolution_order_and_versioning() {
    if skip_unless_enabled() {
        eprintln!("skipping it_ai_anomaly_chat (set MS_RUN_IT=1)");
        return;
    }
    let store = boot().await;
    let repo = PgAgentPromptRepository::new(store.pool.clone());
    let org = Id::from_string("orgA");
    let user = Id::from_string("userA");

    // builtin seed 存在：resolve(anomaly_analysis) 命中 builtin。
    let r0 = repo
        .resolve(&org, &user, "anomaly_analysis")
        .await
        .expect("resolve builtin");
    assert_eq!(r0.scope, "builtin");
    assert_eq!(r0.builtin_key.as_deref(), Some("analysis.anomaly"));

    // builtin 不可变：update / delete 拒绝。
    assert!(repo.update(r0.clone()).await.is_err());
    assert!(repo.delete(&r0.id).await.is_err());

    // 建 org override + set_default → resolve 命中 org。
    let now = TimestampMicros::now();
    let org_tpl = AgentPromptTemplate {
        id: Id::from_string("org-anomaly"),
        org_id: Some(org.0.clone()),
        user_id: None,
        scope: "org".into(),
        builtin_key: Some("analysis.anomaly".into()),
        purpose: "anomaly_analysis".into(),
        name: "Org anomaly".into(),
        body: "org body {{time_range}}".into(),
        variables_schema: json!({"type":"object","properties":{"time_range":{}}}),
        is_default: false,
        enabled: true,
        version: 1,
        parent_id: Some(r0.id.0.clone()),
        created_by: Some(user.0.clone()),
        updated_by: Some(user.0.clone()),
        created_at: now,
        updated_at: now,
    };
    let created = repo.create(org_tpl).await.expect("create org");
    repo.set_default(&created.id)
        .await
        .expect("set org default");
    let r1 = repo
        .resolve(&org, &user, "anomaly_analysis")
        .await
        .expect("resolve org");
    assert_eq!(r1.scope, "org");
    assert_eq!(r1.id.0, "org-anomaly");

    // update override 递增 version。
    let mut to_update = created.clone();
    to_update.body = "org body v2 {{time_range}}".into();
    let updated = repo.update(to_update).await.expect("update org");
    assert_eq!(updated.version, 2);

    // 建 user override + set_default → resolve 命中 user（最高优先级）。
    let user_tpl = AgentPromptTemplate {
        id: Id::from_string("user-anomaly"),
        org_id: Some(org.0.clone()),
        user_id: Some(user.0.clone()),
        scope: "user".into(),
        builtin_key: Some("analysis.anomaly".into()),
        purpose: "anomaly_analysis".into(),
        name: "User anomaly".into(),
        body: "user body {{time_range}}".into(),
        variables_schema: json!({"type":"object","properties":{"time_range":{}}}),
        is_default: false,
        enabled: true,
        version: 1,
        parent_id: Some(r0.id.0.clone()),
        created_by: Some(user.0.clone()),
        updated_by: Some(user.0.clone()),
        created_at: now,
        updated_at: now,
    };
    let u = repo.create(user_tpl).await.expect("create user");
    repo.set_default(&u.id).await.expect("set user default");
    let r2 = repo
        .resolve(&org, &user, "anomaly_analysis")
        .await
        .expect("resolve user");
    assert_eq!(r2.scope, "user");
    assert_eq!(r2.id.0, "user-anomaly");

    // 另一个 user 不受影响（user override 隔离）→ 仍命中 org。
    let other_user = Id::from_string("userB");
    let r3 = repo
        .resolve(&org, &other_user, "anomaly_analysis")
        .await
        .expect("resolve other");
    assert_eq!(r3.scope, "org");
}

#[tokio::test]
async fn archive_metadata_and_retention() {
    if skip_unless_enabled() {
        eprintln!("skipping it_ai_anomaly_chat (set MS_RUN_IT=1)");
        return;
    }
    let store = boot().await;
    let repo = PgChatArchiveRepository::new(store.pool.clone());
    let org = Id::from_string("orgA");
    let chat = Id::from_string("chat1");

    // ok 归档。
    repo.record(ChatArchive {
        id: Id::from_string("arch-ok"),
        chat_id: chat.clone(),
        org_id: org.clone(),
        object_key: Some("intelligence/chat/orgA/chat1/transcript.json".into()),
        sha256: Some("deadbeef".into()),
        bytes: 1234,
        status: "ok".into(),
        error: None,
        created_by: Some("userA".into()),
        created_at: TimestampMicros(1_000),
    })
    .await
    .expect("record ok");

    // failed 归档（无 object_key）。
    repo.record(ChatArchive {
        id: Id::from_string("arch-fail"),
        chat_id: chat.clone(),
        org_id: org.clone(),
        object_key: None,
        sha256: None,
        bytes: 0,
        status: "failed".into(),
        error: Some("object store down".into()),
        created_by: Some("userA".into()),
        created_at: TimestampMicros(2_000),
    })
    .await
    .expect("record fail");

    let list = repo.list_for_chat(&chat).await.expect("list");
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].status, "failed"); // newest first

    // retention：删 created_at < 1_500 → 只删 arch-ok，返回它的 object_key。
    let keys = repo.delete_older_than(1_500).await.expect("retention");
    assert_eq!(
        keys,
        vec!["intelligence/chat/orgA/chat1/transcript.json".to_string()]
    );
    let after = repo.list_for_chat(&chat).await.expect("list2");
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].id.0, "arch-fail");
}

#[tokio::test]
async fn chat_soft_delete() {
    if skip_unless_enabled() {
        eprintln!("skipping it_ai_anomaly_chat (set MS_RUN_IT=1)");
        return;
    }
    let store = boot().await;
    let repo = PgChatRepository::new(store.pool.clone());
    let org = Id::from_string("orgA");
    let user = Id::from_string("userA");
    let id = Id::from_string("chat1");

    let s = Chat::minimal(
        id.clone(),
        org.clone(),
        user.clone(),
        "openai".into(),
        "gpt-4o".into(),
        "investigation".into(),
        TimestampMicros::now(),
    );
    repo.create_chat(s).await.expect("create");

    assert_eq!(repo.list_chats(&org, &user).await.expect("list").len(), 1);

    // 软删后：normal 列表过滤、get_chat 报错、get_chat_any 仍可解析。
    repo.delete_chat(&org, &id).await.expect("delete");
    assert_eq!(repo.list_chats(&org, &user).await.expect("list2").len(), 0);
    assert!(repo.get_chat(&org, &id).await.is_err());
    let any = repo.get_chat_any(&org, &id).await.expect("get any");
    assert!(any.deleted_at_micros.is_some());
}

fn audit_event(org: &Id, id: &str, action: &str, target_kind: &str, ts: i64) -> AuditEvent {
    AuditEvent {
        id: Id::from_string(id),
        org_id: org.clone(),
        actor_kind: "user".into(),
        actor_id: "userA".into(),
        action: action.into(),
        target_kind: Some(target_kind.into()),
        target_id: Some("t1".into()),
        ip: None,
        user_agent: None,
        payload: json!({}),
        ts: TimestampMicros(ts),
    }
}

#[tokio::test]
async fn audit_query_filters_and_cursor_stability() {
    if skip_unless_enabled() {
        eprintln!("skipping it_ai_anomaly_chat (set MS_RUN_IT=1)");
        return;
    }
    let store = boot().await;
    let repo = PgAuditEventRepository::new(store.pool.clone());
    let org = Id::from_string("orgA");
    let other = Id::from_string("orgB");

    // 5 条本 org（不同 ts/action/target_kind）+ 1 条别的 org。
    for (i, (action, tk)) in [
        ("ai.provider.create", "ai_provider"),
        ("ai.prompt.update", "ai_prompt"),
        ("ai.provider.rotate_key", "ai_provider"),
        ("ai.prompt.set_default", "ai_prompt"),
        ("intelligence.chat.archived", "intelligence_chat"),
    ]
    .iter()
    .enumerate()
    {
        repo.record(audit_event(
            &org,
            &format!("e{i}"),
            action,
            tk,
            1_000 + i as i64,
        ))
        .await
        .expect("record");
    }
    repo.record(audit_event(
        &other,
        "other",
        "ai.provider.create",
        "ai_provider",
        9_999,
    ))
    .await
    .expect("record other");

    // 过滤 action：只命中本 org 对应行。
    let q = AuditQuery {
        action: Some("ai.provider.rotate_key".into()),
        limit: 50,
        ..Default::default()
    };
    let rows = repo.query(&org, &q).await.expect("query action");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].action, "ai.provider.rotate_key");

    // 过滤 target_kind：两条 ai_prompt。
    let q = AuditQuery {
        target_kind: Some("ai_prompt".into()),
        limit: 50,
        ..Default::default()
    };
    let rows = repo.query(&org, &q).await.expect("query tk");
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|r| r.target_kind.as_deref() == Some("ai_prompt"))
    );

    // org 隔离：orgB 的行不会出现在 orgA 查询里。
    let q = AuditQuery {
        limit: 50,
        ..Default::default()
    };
    let all = repo.query(&org, &q).await.expect("query all");
    assert_eq!(all.len(), 5);
    assert!(all.iter().all(|r| r.org_id.0 == "orgA"));
    // DESC 排序：最新 ts 在前。
    assert_eq!(all[0].ts.0, 1_004);

    // 游标分页：page1 limit=2 → 探测多取一行后截断为 2，next_cursor 取第 2 行。
    let page1 = repo
        .query(
            &org,
            &AuditQuery {
                limit: 3,
                ..Default::default()
            },
        )
        .await
        .expect("page1");
    // 模拟路由：page_size=2，多取 1 行探测。
    assert!(page1.len() == 3);
    let cursor = (page1[1].ts.0, page1[1].id.0.clone());
    let page2 = repo
        .query(
            &org,
            &AuditQuery {
                limit: 3,
                cursor: Some(cursor),
                ..Default::default()
            },
        )
        .await
        .expect("page2");
    // page2 第一行严格早于 page1 第二行，无重叠。
    assert!(
        page2[0].ts.0 < page1[1].ts.0
            || (page2[0].ts.0 == page1[1].ts.0 && page2[0].id.0 < page1[1].id.0)
    );
    let p1_ids: Vec<_> = page1[..2].iter().map(|r| r.id.0.clone()).collect();
    assert!(
        page2.iter().all(|r| !p1_ids.contains(&r.id.0)),
        "no overlap across pages"
    );
}
