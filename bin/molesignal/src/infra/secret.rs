// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `secret_ref` 指针解析（spec federated-search）。
//!
//! `RemoteCluster.token_secret_ref` 是个指针而非明文，支持：
//! - `env:VAR_NAME` → 读环境变量（spec 示例 `env:OBS_SF_TOKEN`）。
//! - `cipher_keys:<ref_id>` → 从 [`ClusterSecretRepository`]（`cluster_secrets` 表，root key
//!   envelope 加密）解出明文 token。
//!
//! 解析出的明文（bearer token）**绝不能**进日志或任何序列化响应。

use crate::{
    infra::cluster::cluster_secrets_repo::ClusterSecretRepository,
    shared::{Error, Result, ids::Id},
};

/// 把 `secret_ref` 指针解析成明文 secret。
///
/// `env:` 分支不需要 `secrets`（传 `None` 即可）；`cipher_keys:` 分支需要注入
/// `secrets` 存储，未注入时返错（OSS 本地 / 纯 env 部署不需要 DB 后端）。
pub async fn resolve_secret_ref(
    secret_ref: &str,
    org_id: &Id,
    secrets: Option<&dyn ClusterSecretRepository>,
) -> Result<String> {
    if let Some(var) = secret_ref.strip_prefix("env:") {
        if var.is_empty() {
            return Err(Error::invalid("secret_ref `env:` 缺少环境变量名"));
        }
        return std::env::var(var)
            .map_err(|_| Error::internal(format!("secret 环境变量 `{var}` 未设置")));
    }
    if let Some(ref_id) = secret_ref.strip_prefix("cipher_keys:") {
        if ref_id.is_empty() {
            return Err(Error::invalid("secret_ref `cipher_keys:` 缺少 ref_id"));
        }
        let repo = secrets.ok_or_else(|| {
            Error::internal(
                "secret_ref `cipher_keys:` 需要 cluster-secrets 存储但未注入；\
                 OSS 本地 / 纯 env 部署请改用 `env:VAR_NAME`",
            )
        })?;
        return repo.get_plaintext(org_id, ref_id).await;
    }
    Err(Error::invalid(format!(
        "不支持的 secret_ref `{secret_ref}`（期望 `env:VAR_NAME` 或 `cipher_keys:<ref_id>`）"
    )))
}

/// 为**集群级控制 RPC**（gossip / CancelQuery）解析一个可用 bearer token。
///
/// 这类 RPC 非 org 维度，但 `cluster_secrets` 是按 `(org_id, ref_id)` 存的、`cipher_keys:`
/// 解析需要 org 上下文。接收端只要求 bearer 是**合法 api token**（证明是已知 peer），
/// 故任一有 org 上下文的 per-org link token 即可用。解析顺序：
/// 1. 各 per-org link 的 `token_secret_ref`（带其 `local_org_id`，`cipher_keys:` 可解）；
/// 2. 集群级 `token_secret_ref`：`env:` 与 org 无关直接解；`cipher_keys:` 再试各 linked org。
///
/// 全失败返 `None`（调用方软降级跳过该 peer）。
pub async fn resolve_cluster_control_token(
    cluster_token_ref: &str,
    per_org_refs: &[(Id, Option<String>)],
    secrets: Option<&dyn ClusterSecretRepository>,
) -> Option<String> {
    // 1. per-org link token（org 上下文明确）。
    for (org, tref) in per_org_refs {
        if let Some(tref) = tref
            && !tref.trim().is_empty()
            && let Ok(tok) = resolve_secret_ref(tref, org, secrets).await
        {
            return Some(tok);
        }
    }
    // 2. 集群级 token：env: / plain 与 org 无关（空 org 即可）；cipher_keys: 试各 linked org。
    if !cluster_token_ref.trim().is_empty() {
        if let Ok(tok) = resolve_secret_ref(cluster_token_ref, &Id(String::new()), secrets).await {
            return Some(tok);
        }
        for (org, _) in per_org_refs {
            if let Ok(tok) = resolve_secret_ref(cluster_token_ref, org, secrets).await {
                return Some(tok);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;

    /// 内存桩 repo：只为验证 `cipher_keys:` 分支被正确路由到 repo，无需 DB。
    struct StubSecrets {
        ref_id: String,
        plaintext: String,
    }

    #[async_trait]
    impl ClusterSecretRepository for StubSecrets {
        async fn put(&self, _org_id: &Id, _ref_id: &str, _plaintext: &str) -> Result<()> {
            Ok(())
        }
        async fn get_plaintext(&self, _org_id: &Id, ref_id: &str) -> Result<String> {
            if ref_id == self.ref_id {
                Ok(self.plaintext.clone())
            } else {
                Err(Error::not_found(format!("no secret `{ref_id}`")))
            }
        }
        async fn delete(&self, _org_id: &Id, _ref_id: &str) -> Result<()> {
            Ok(())
        }
    }

    fn org() -> Id {
        Id::from_string("orgA")
    }

    #[tokio::test]
    async fn env_scheme_reads_var() {
        // SAFETY: 单测内独占设置/清理，无并发读写同名 var。
        unsafe { std::env::set_var("MS_TEST_SECRET_REF_VAR", "s3cr3t") };
        let got = resolve_secret_ref("env:MS_TEST_SECRET_REF_VAR", &org(), None)
            .await
            .unwrap();
        assert_eq!(got, "s3cr3t");
        unsafe { std::env::remove_var("MS_TEST_SECRET_REF_VAR") };
    }

    #[tokio::test]
    async fn env_missing_var_errors() {
        assert!(
            resolve_secret_ref("env:MS_DEFINITELY_UNSET_VAR_XYZ", &org(), None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn empty_env_name_errors() {
        assert!(resolve_secret_ref("env:", &org(), None).await.is_err());
    }

    #[tokio::test]
    async fn cipher_keys_scheme_routes_to_repo() {
        let stub = StubSecrets {
            ref_id: "sf-token".into(),
            plaintext: "Bearer-material".into(),
        };
        let got = resolve_secret_ref("cipher_keys:sf-token", &org(), Some(&stub))
            .await
            .unwrap();
        assert_eq!(got, "Bearer-material");
    }

    #[tokio::test]
    async fn cipher_keys_unknown_ref_errors() {
        let stub = StubSecrets {
            ref_id: "sf-token".into(),
            plaintext: "x".into(),
        };
        assert!(
            resolve_secret_ref("cipher_keys:nope", &org(), Some(&stub))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn cipher_keys_without_repo_errors() {
        // 未注入 secrets 存储 → 明确报错而非静默。
        assert!(
            resolve_secret_ref("cipher_keys:sf-token", &org(), None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn empty_cipher_keys_id_errors() {
        assert!(
            resolve_secret_ref("cipher_keys:", &org(), None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn unknown_scheme_errors() {
        assert!(
            resolve_secret_ref("file:/etc/token", &org(), None)
                .await
                .is_err()
        );
        assert!(
            resolve_secret_ref("plain-token", &org(), None)
                .await
                .is_err()
        );
    }
}
