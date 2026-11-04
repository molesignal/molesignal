// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 所有节点共用数据库里的 JWT 签名 secret，每个节点在内存里留一份。
//!
//! 一个节点轮换之后，其余节点不用重启，靠两条路跟上：
//! - 定期重读（`bootstrap/workers/jwt_secrets.rs`）：换上新的 primary，不再认过了宽限期的 secret。
//! - 收到签名对不上的 token 时马上重读，再校验一次：轮换后的第一个请求就认得，不会因为这边还没读到，
//!   就把刚在别的节点登录的人退出登录。
//!
//! 谁都能发一个签名不对的 token，所以每个节点最多每 [`MIN_READ_GAP`] 读一次库：签名对不上的请求
//! 排队等下一次读，共用它的结果。签名对得上的请求不读库，也不排队。

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use jsonwebtoken::{DecodingKey, Validation, decode, errors::ErrorKind};
use serde::de::IgnoredAny;
use tokio::{sync::Mutex, time::Instant};

use super::IamService;
use crate::shared::{Error, Result};

/// 两次读库至少隔这么久。
const MIN_READ_GAP: Duration = Duration::from_millis(500);

/// 所有节点共用的 secret 集合存在哪里。
#[async_trait]
pub trait JwtSecretSource: Send + Sync {
    /// `[0]` 是 primary（用来签新 token），其余是宽限期内的 retired secret（只参与校验）。
    async fn active_jwt_secrets(&self) -> Result<Vec<Vec<u8>>>;
}

pub(super) struct JwtSecretReads {
    source: Arc<dyn JwtSecretSource>,
    /// 同一时间只有一次读，读到的集合按先后换上，慢的一次盖不掉后来读到的。
    history: Mutex<ReadHistory>,
}

#[derive(Default)]
struct ReadHistory {
    /// 最近一次读开始的时间。请求断开时读到一半的那次也算，限速不会被断开绕过。
    began: Option<Instant>,
    /// 最近读完的一次：开始的时间，读到了没有。
    finished: Option<(Instant, bool)>,
}

impl IamService {
    /// 从这里重读 secret 集合。不设时（单测）一直用构造时给的集合。
    pub fn with_jwt_secret_source(mut self, source: Arc<dyn JwtSecretSource>) -> Self {
        self.jwt_secret_reads = Some(JwtSecretReads {
            source,
            history: Mutex::default(),
        });
        self
    }

    /// 读库并换上，返回现在参与校验的 secret 数量。读不出时保持当前的集合。
    pub async fn reload_jwt_secrets(&self) -> Result<usize> {
        let Some(reads) = &self.jwt_secret_reads else {
            return Ok(self.jwt_secrets.read().len());
        };

        let mut history = reads.history.lock().await;

        self.read_jwt_secrets(reads, &mut history).await
    }

    /// 校验一个可能由别的节点签发的 token。`verify` 失败而且是签名对不上时，这边可能还没读到别的
    /// 节点的轮换，读到之后再校验一次。
    ///
    /// 读不出时回 [`Error::Unavailable`]：这个 token 可能是好的，不能说它无效，否则刷新接口会把
    /// 浏览器的会话清掉。
    pub async fn verify_with_current_secrets<T>(
        &self,
        token: &str,
        verify: impl Fn(&Self) -> Result<T>,
    ) -> Result<T> {
        let arrived = Instant::now();
        let verified = verify(self);

        if verified.is_ok() || !self.signature_is_unknown(token) {
            return verified;
        }

        self.read_jwt_secrets_since(arrived).await?;

        verify(self)
    }

    /// 手里的 secret 都对不上这个 token 的签名。过期、用途不对或者格式不对都不算：过期说明签名
    /// 对得上，格式不对的 token 哪个节点也签不出来。
    fn signature_is_unknown(&self, token: &str) -> bool {
        let mut validation = Validation::default();
        validation.validate_exp = false;
        validation.validate_aud = false;
        validation.required_spec_claims.clear();

        self.jwt_secrets.read().iter().all(|secret| {
            decode::<IgnoredAny>(token, &DecodingKey::from_secret(secret), &validation)
                .is_err_and(|error| matches!(error.kind(), ErrorKind::InvalidSignature))
        })
    }

    /// 确保手里的集合来自一次在 `arrived` 之后才开始的读：别的节点在这之前轮换的都读得到。
    async fn read_jwt_secrets_since(&self, arrived: Instant) -> Result<()> {
        let Some(reads) = &self.jwt_secret_reads else {
            return Ok(());
        };

        let mut history = reads.history.lock().await;

        // 排队时已经有人替这个请求读过了。
        if let Some((began, read)) = history.finished
            && began >= arrived
        {
            return if read { Ok(()) } else { Err(unreadable()) };
        }

        if let Some(began) = history.began {
            tokio::time::sleep_until(began + MIN_READ_GAP).await;
        }

        match self.read_jwt_secrets(reads, &mut history).await {
            Ok(_) => Ok(()),
            Err(error) => {
                tracing::warn!(%error, "JWT signing secrets unreadable while checking a token");

                Err(unreadable())
            }
        }
    }

    async fn read_jwt_secrets(
        &self,
        reads: &JwtSecretReads,
        history: &mut ReadHistory,
    ) -> Result<usize> {
        let began = Instant::now();
        history.began = Some(began);

        let active = reads.source.active_jwt_secrets().await.and_then(|active| {
            if active.is_empty() {
                return Err(Error::internal("the JWT signing secret set is empty"));
            }

            Ok(active)
        });

        history.finished = Some((began, active.is_ok()));

        let active = active?;
        let count = active.len();

        if *self.jwt_secrets.read() != active {
            *self.jwt_secrets.write() = active;

            tracing::info!(
                active_count = count,
                "JWT signing secrets changed in the database; using the new set"
            );
        }

        Ok(count)
    }
}

fn unreadable() -> Error {
    Error::unavailable("the signing secrets cannot be read right now; try again")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use futures::future::join_all;
    use parking_lot::Mutex as SyncMutex;
    use tokio::sync::Notify;

    use super::*;
    use crate::{
        app::iam::{SignInMethod, SignInSession, test_support::iam_service},
        domain::iam::IamScope,
        shared::ids::Id,
    };

    const OLD: &[u8] = b"secret-before-the-rotation";
    const NEW: &[u8] = b"secret-after-the-rotation";

    /// 所有节点共用的那张表。
    #[derive(Default)]
    struct Table {
        secrets: SyncMutex<Vec<Vec<u8>>>,
        reads: AtomicUsize,
        unreadable: AtomicBool,
        /// 下一次读先记下当时的集合，等到信号才返回。
        hold_next_read: SyncMutex<Option<Arc<Notify>>>,
    }

    impl Table {
        fn holding(secrets: &[&[u8]]) -> Arc<Self> {
            let table = Arc::new(Self::default());
            table.set(secrets);
            table
        }

        fn set(&self, secrets: &[&[u8]]) {
            *self.secrets.lock() = secrets.iter().map(|secret| secret.to_vec()).collect();
        }

        fn reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }

        fn hold_next_read(&self) -> Arc<Notify> {
            let gate = Arc::new(Notify::new());
            *self.hold_next_read.lock() = Some(gate.clone());
            gate
        }
    }

    #[async_trait]
    impl JwtSecretSource for Table {
        async fn active_jwt_secrets(&self) -> Result<Vec<Vec<u8>>> {
            self.reads.fetch_add(1, Ordering::SeqCst);

            let read = self.secrets.lock().clone();
            let gate = self.hold_next_read.lock().take();

            if let Some(gate) = gate {
                gate.notified().await;
            }

            if self.unreadable.load(Ordering::SeqCst) {
                return Err(Error::internal("connection refused"));
            }

            Ok(read)
        }
    }

    fn holding(secrets: &[&[u8]]) -> IamService {
        let iam = iam_service(86_400);
        *iam.jwt_secrets.write() = secrets.iter().map(|secret| secret.to_vec()).collect();
        iam
    }

    /// 一个刚读过这张表的节点。
    async fn node_on(table: &Arc<Table>) -> IamService {
        let node = holding(&[b"not-read-yet"]).with_jwt_secret_source(table.clone());
        node.reload_jwt_secrets().await.unwrap();
        node
    }

    fn session(node: &IamService) -> SignInSession {
        node.begin_session(
            &Id::from_string("user-1"),
            &Id::from_string("org-1"),
            IamScope::Organization,
            false,
            SignInMethod::single_factor(),
        )
    }

    fn access_token(node: &IamService) -> String {
        node.issue_access_token(&session(node)).unwrap().0
    }

    fn forged_token() -> String {
        access_token(&holding(&[b"nobody-holds-this-secret"]))
    }

    async fn accepts(node: &IamService, token: &str) -> bool {
        node.verify_with_current_secrets(token, |iam| iam.verify_token(token))
            .await
            .is_ok()
    }

    /// 这个节点签发的 token 是不是用 `secret` 签的。
    fn signs_with(node: &IamService, secret: &[u8]) -> bool {
        holding(&[secret]).verify_token(&access_token(node)).is_ok()
    }

    #[tokio::test]
    async fn a_session_signed_right_after_a_rotation_is_accepted_by_a_node_that_has_not_read_it() {
        let table = Table::holding(&[OLD]);
        let rotated = node_on(&table).await;
        let behind = node_on(&table).await;

        table.set(&[NEW, OLD]);
        rotated.reload_jwt_secrets().await.unwrap();

        let tokens = rotated.issue_session_tokens(&session(&rotated)).unwrap();

        assert!(behind.verify_token(&tokens.access_token).is_err());
        assert!(accepts(&behind, &tokens.access_token).await);
        assert!(
            behind
                .verify_with_current_secrets(&tokens.refresh_token, |iam| {
                    iam.verify_refresh_token(&tokens.refresh_token)
                })
                .await
                .is_ok()
        );
        assert!(signs_with(&behind, NEW), "it signs with what it just read");
    }

    #[tokio::test]
    async fn a_node_that_has_not_read_a_rotation_signs_sessions_the_others_accept() {
        let table = Table::holding(&[OLD]);
        let rotated = node_on(&table).await;
        let behind = node_on(&table).await;

        table.set(&[NEW, OLD]);
        rotated.reload_jwt_secrets().await.unwrap();

        let reads = table.reads();
        let token = access_token(&behind);

        assert!(signs_with(&behind, OLD));
        assert!(accepts(&rotated, &token).await);
        assert_eq!(table.reads(), reads, "a retired secret needs no read");
    }

    #[tokio::test]
    async fn reloading_moves_signing_to_the_new_primary_without_any_token_arriving() {
        let table = Table::holding(&[OLD]);
        let node = node_on(&table).await;

        table.set(&[NEW, OLD]);

        assert!(signs_with(&node, OLD));
        assert_eq!(node.reload_jwt_secrets().await.unwrap(), 2);
        assert!(signs_with(&node, NEW));
    }

    #[tokio::test]
    async fn past_the_grace_the_retired_secret_is_dropped_and_every_node_accepts_the_others() {
        let table = Table::holding(&[NEW, OLD]);
        let a = node_on(&table).await;
        let b = node_on(&table).await;
        let signed_before_the_rotation = access_token(&holding(&[OLD]));

        assert!(a.verify_token(&signed_before_the_rotation).is_ok());
        assert!(b.verify_token(&signed_before_the_rotation).is_ok());

        table.set(&[NEW]);
        a.reload_jwt_secrets().await.unwrap();
        b.reload_jwt_secrets().await.unwrap();

        assert!(!accepts(&a, &signed_before_the_rotation).await);
        assert!(!accepts(&b, &signed_before_the_rotation).await);
        assert!(a.verify_token(&access_token(&b)).is_ok());
        assert!(b.verify_token(&access_token(&a)).is_ok());
    }

    #[tokio::test]
    async fn only_an_unknown_signature_reads_the_database() {
        let table = Table::holding(&[NEW, OLD]);
        let node = node_on(&table).await;
        let reads = table.reads();

        let mut ended = session(&node);
        ended.expires_at = chrono::Utc::now().timestamp() - 3_600;
        let expired = node.issue_access_token(&ended).unwrap().0;
        let refresh = node
            .issue_session_tokens(&session(&node))
            .unwrap()
            .refresh_token;

        assert!(accepts(&node, &access_token(&node)).await);
        assert!(accepts(&node, &access_token(&holding(&[OLD]))).await);
        assert!(!accepts(&node, &expired).await);
        assert!(!accepts(&node, &refresh).await);
        assert!(!accepts(&node, "not a token").await);
        assert!(!accepts(&node, "a.b.c").await);
        assert_eq!(table.reads(), reads);
    }

    #[tokio::test]
    async fn unknown_signatures_share_reads_and_keep_to_the_gap() {
        let start = Instant::now();
        let table = Table::holding(&[NEW]);
        let node = node_on(&table).await;
        let forged = forged_token();

        let wave = || {
            join_all((0..50).map(|_| {
                node.verify_with_current_secrets(&forged, |iam| iam.verify_token(&forged))
            }))
        };

        for result in wave().await {
            assert!(matches!(result, Err(Error::Unauthorized(_))));
        }

        assert_eq!(table.reads(), 2, "the load, then one read for the wave");

        for result in wave().await {
            assert!(matches!(result, Err(Error::Unauthorized(_))));
        }

        assert_eq!(table.reads(), 3);
        assert!(start.elapsed() >= MIN_READ_GAP * 2);
    }

    #[tokio::test]
    async fn a_read_cancelled_with_its_request_still_counts_towards_the_gap() {
        let start = Instant::now();
        let table = Table::holding(&[NEW]);
        let node = node_on(&table).await;
        let forged = forged_token();
        let _gate = table.hold_next_read();

        tokio::time::timeout(
            MIN_READ_GAP + Duration::from_millis(100),
            accepts(&node, &forged),
        )
        .await
        .expect_err("the read is held");

        assert_eq!(table.reads(), 2, "it was reading when it was dropped");
        assert!(!accepts(&node, &forged).await);
        assert_eq!(table.reads(), 3);
        assert!(start.elapsed() >= MIN_READ_GAP * 2);
    }

    #[tokio::test]
    async fn an_unreadable_database_does_not_make_a_token_invalid() {
        let table = Table::holding(&[OLD]);
        let node = node_on(&table).await;
        let token = access_token(&holding(&[NEW]));

        table.unreadable.store(true, Ordering::SeqCst);

        let answer = node
            .verify_with_current_secrets(&token, |iam| iam.verify_token(&token))
            .await;

        assert!(matches!(answer, Err(Error::Unavailable(_))));

        // 排队共用一次读的请求也一样，读不出时不各自再读。
        let reads = table.reads();

        let wave = join_all(
            (0..5)
                .map(|_| node.verify_with_current_secrets(&token, |iam| iam.verify_token(&token))),
        )
        .await;

        for answer in wave {
            assert!(matches!(answer, Err(Error::Unavailable(_))));
        }

        assert_eq!(table.reads(), reads + 1);
    }

    #[tokio::test]
    async fn an_unreadable_database_keeps_the_current_secrets() {
        let table = Table::holding(&[OLD]);
        let node = node_on(&table).await;
        let token = access_token(&node);

        table.unreadable.store(true, Ordering::SeqCst);

        assert!(node.reload_jwt_secrets().await.is_err());
        assert!(node.verify_token(&token).is_ok());
        assert!(signs_with(&node, OLD));

        table.unreadable.store(false, Ordering::SeqCst);
        table.set(&[]);

        assert!(
            node.reload_jwt_secrets().await.is_err(),
            "nothing to sign with"
        );
        assert!(signs_with(&node, OLD));
    }

    #[tokio::test]
    async fn a_slow_read_cannot_overwrite_a_later_one() {
        let table = Table::holding(&[OLD]);
        let node = Arc::new(node_on(&table).await);
        let gate = table.hold_next_read();

        let slow = tokio::spawn({
            let node = node.clone();

            async move { node.reload_jwt_secrets().await }
        });

        while table.reads() < 2 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        table.set(&[NEW, OLD]);

        let later = tokio::spawn({
            let node = node.clone();

            async move { node.reload_jwt_secrets().await }
        });

        tokio::time::sleep(Duration::from_millis(20)).await;
        gate.notify_one();

        assert_eq!(slow.await.unwrap().unwrap(), 1);
        assert_eq!(later.await.unwrap().unwrap(), 2);
        assert!(signs_with(&node, NEW));
    }

    #[tokio::test]
    async fn a_node_without_a_source_keeps_what_it_was_given() {
        let node = holding(&[OLD]);
        let token = access_token(&node);

        assert_eq!(node.reload_jwt_secrets().await.unwrap(), 1);
        assert!(accepts(&node, &token).await);
        assert!(!accepts(&node, &forged_token()).await);
    }
}
