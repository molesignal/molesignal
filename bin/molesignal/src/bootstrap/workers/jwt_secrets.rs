// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! JWT 签名 secret：每个节点启动时读一次（见 `bootstrap/iam.rs`），之后每隔 10 秒再读；轮换的节点
//! 在接口里立即换上（见 `routes/jwt_secrets.rs`）。读不出时保持当前的集合。
//!
//! 这里决定节点多快开始用新的 primary 签发，以及什么时候不再认过了宽限期的 secret。校验不用等它：
//! 签名对不上的 token 会让节点马上重读（见 `app/iam/jwt_secrets.rs`）。

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use tokio::task::JoinHandle;

use crate::{
    app::iam::{IamService, JwtSecretSource},
    infra::persistence::repositories::signing_secrets::{
        SigningSecretRepository, load_active_jwt_secrets,
    },
    shared::Result,
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(10);

/// 所有节点共用的 `signing_secrets` 表。
pub struct SigningSecretsTable(pub Arc<dyn SigningSecretRepository>);

#[async_trait]
impl JwtSecretSource for SigningSecretsTable {
    async fn active_jwt_secrets(&self) -> Result<Vec<Vec<u8>>> {
        load_active_jwt_secrets(self.0.as_ref()).await
    }
}

pub fn spawn(iam: Arc<IamService>) -> JoinHandle<()> {
    refresh_every(iam, REFRESH_INTERVAL)
}

fn refresh_every(iam: Arc<IamService>, interval: Duration) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);

        loop {
            ticker.tick().await;

            if let Err(error) = iam.reload_jwt_secrets().await {
                tracing::warn!(%error, "JWT signing secrets unreadable; keeping the current set");
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use parking_lot::Mutex;

    use super::*;
    use crate::{
        app::iam::test_support::iam_service,
        shared::{Error, ids::Id},
    };

    struct Table {
        secrets: Mutex<Vec<Vec<u8>>>,
        unreadable: AtomicBool,
    }

    #[async_trait]
    impl JwtSecretSource for Table {
        async fn active_jwt_secrets(&self) -> Result<Vec<Vec<u8>>> {
            if self.unreadable.load(Ordering::SeqCst) {
                return Err(Error::internal("connection refused"));
            }

            Ok(self.secrets.lock().clone())
        }
    }

    fn token(iam: &IamService) -> String {
        iam.issue_token(&Id::from_string("user-1"), &Id::from_string("org-1"))
            .unwrap()
    }

    #[tokio::test]
    async fn the_worker_picks_up_a_rotation_and_survives_an_unreadable_table() {
        let table = Arc::new(Table {
            secrets: Mutex::new(vec![b"before".to_vec()]),
            unreadable: AtomicBool::new(true),
        });

        let node = Arc::new(iam_service(3_600).with_jwt_secret_source(table.clone()));
        let signed_at_start = token(&node);
        let worker = refresh_every(node.clone(), Duration::from_millis(10));

        tokio::time::sleep(Duration::from_millis(50)).await;

        assert!(
            node.verify_token(&signed_at_start).is_ok(),
            "still the set it had"
        );
        assert!(!worker.is_finished());

        *table.secrets.lock() = vec![b"after".to_vec(), b"before".to_vec()];
        table.unreadable.store(false, Ordering::SeqCst);

        for _ in 0..100 {
            if node.verify_token(&signed_at_start).is_err() {
                break;
            }

            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert!(
            node.verify_token(&signed_at_start).is_err(),
            "the table's set replaced it"
        );

        worker.abort();
    }
}
