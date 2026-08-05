// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ACME (RFC 8555) HTTP-01 客户端，基于 `instant-acme`（change `domain-acme-tls`）。
//!
//! 流程（每个 `issue(hostname)` 调用）：
//! 1. 取/建账户（`Account::create` 或 `from_credentials` 复用 `key_storage_dir/account.json`）
//! 2. `account.new_order` with DNS identifier
//! 3. `authorizations()` → 取 http-01 challenge → `key_authorization`
//! 4. **写入 `acme_challenges` 表**：let `/.well-known/acme-challenge/<token>` handler 暴露给 LE
//! 5. `set_challenge_ready(challenge.url)` → 轮询 `order.refresh()` 直到 `Ready`
//! 6. 本地 `rcgen` 生 ECDSA p256 key + CSR → `order.finalize(csr_der)`
//! 7. 轮询直到 `Valid` → `order.certificate()` 拿 cert chain PEM
//! 8. 写 cert_pem 到 `domains.cert_pem`；key_pem 落 `key_storage_dir/<hostname>.key.pem`

use std::{path::PathBuf, sync::Arc, time::Duration};

use instant_acme::{
    Account, AccountCredentials, AuthorizationStatus, ChallengeType, Identifier, NewAccount,
    NewOrder, RetryPolicy,
};

use crate::{
    infra::persistence::repositories::domains::{AcmeChallenge, DomainRepository},
    shared::{ids::Id, time::TimestampMicros},
};

#[derive(Debug, thiserror::Error)]
pub enum AcmeError {
    #[error("acme io: {0}")]
    Io(#[from] std::io::Error),
    #[error("acme json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("acme protocol: {0}")]
    Protocol(#[from] instant_acme::Error),
    #[error("acme rcgen: {0}")]
    Rcgen(#[from] rcgen::Error),
    #[error("acme: no http-01 challenge available for {0}")]
    NoHttp01Challenge(String),
    #[error("acme: order failed for {0}: {1}")]
    OrderFailed(String, String),
    #[error("acme: certificate not yet available after polling")]
    CertNotReady,
    #[error("acme: db error: {0}")]
    Repo(#[from] crate::shared::Error),
}

/// 单次签发出来的产物。`cert_pem` 落 DB，`key_pem` 落本地磁盘。
#[derive(Debug, Clone)]
pub struct IssuedCert {
    pub hostname: String,
    pub cert_pem: String,
    pub key_pem: String,
    pub not_after_micros: i64,
}

/// ACME 客户端。线程安全，可以 clone 给多个 issue/renewal 任务复用。
#[derive(Clone)]
pub struct AcmeClient {
    inner: Arc<AcmeClientInner>,
}

struct AcmeClientInner {
    directory_url: String,
    contact_email: String,
    key_storage_dir: PathBuf,
    domains: Arc<dyn DomainRepository>,
    /// 进程内 cached account；首次 `issue` 时初始化。
    account: tokio::sync::OnceCell<Account>,
}

impl AcmeClient {
    pub fn new(
        directory_url: impl Into<String>,
        contact_email: impl Into<String>,
        key_storage_dir: impl Into<PathBuf>,
        domains: Arc<dyn DomainRepository>,
    ) -> Self {
        Self {
            inner: Arc::new(AcmeClientInner {
                directory_url: directory_url.into(),
                contact_email: contact_email.into(),
                key_storage_dir: key_storage_dir.into(),
                domains,
                account: tokio::sync::OnceCell::new(),
            }),
        }
    }

    /// 复用或新建 ACME account；`account.json` 持久化在 `key_storage_dir/account.json`。
    async fn ensure_account(&self) -> Result<&Account, AcmeError> {
        self.inner
            .account
            .get_or_try_init(|| async {
                let creds_path = self.inner.key_storage_dir.join("account.json");
                if let Ok(bytes) = tokio::fs::read(&creds_path).await {
                    let creds: AccountCredentials = serde_json::from_slice(&bytes)?;
                    return Ok::<_, AcmeError>(Account::builder()?.from_credentials(creds).await?);
                }
                let contact = format!("mailto:{}", self.inner.contact_email);
                let (account, creds) = Account::builder()?
                    .create(
                        &NewAccount {
                            contact: &[&contact],
                            terms_of_service_agreed: true,
                            only_return_existing: false,
                        },
                        self.inner.directory_url.clone(),
                        None,
                    )
                    .await?;
                tokio::fs::write(&creds_path, serde_json::to_vec_pretty(&creds)?).await?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = tokio::fs::metadata(&creds_path).await?.permissions();
                    perms.set_mode(0o600);
                    let _ = tokio::fs::set_permissions(&creds_path, perms).await;
                }
                Ok(account)
            })
            .await
    }

    /// 整条 ACME 流程；返签发好的 cert + key（caller 负责写 DB / 磁盘）。
    ///
    /// `challenge_token_writer` 负责把 `(token, key_authorization)` 持久化（典型实装：
    /// 写 `acme_challenges` 表，让 `/.well-known/acme-challenge/{token}` handler 取到）。
    pub async fn issue(&self, hostname: &str, domain_id: &Id) -> Result<IssuedCert, AcmeError> {
        let account = self.ensure_account().await?;
        let identifiers = vec![Identifier::Dns(hostname.to_string())];
        let mut order = account.new_order(&NewOrder::new(&identifiers)).await?;

        // instant-acme 0.8: authorizations() is now a streamed handle iterator
        // and the challenge ack lives on ChallengeHandle (no manual URL plumbing).
        // We persist each (token, key_auth) so the ACME challenge http handler
        // can answer LE, then notify the server the challenge is ready.
        {
            let mut authorizations = order.authorizations();
            while let Some(authz_res) = authorizations.next().await {
                let mut authz = authz_res?;
                if authz.status == AuthorizationStatus::Valid {
                    continue;
                }
                let mut challenge = authz
                    .challenge(ChallengeType::Http01)
                    .ok_or_else(|| AcmeError::NoHttp01Challenge(hostname.into()))?;
                let key_auth = challenge.key_authorization().as_str().to_string();
                let token = challenge.token.clone();
                self.inner
                    .domains
                    .put_challenge(AcmeChallenge {
                        token,
                        domain_id: domain_id.clone(),
                        key_authorization: key_auth,
                        expires_at: TimestampMicros(
                            chrono::Utc::now().timestamp_micros() + 24 * 3600 * 1_000_000,
                        ),
                    })
                    .await?;
                challenge.set_ready().await?;
            }
        } // release the mut borrow on `order` held by `authorizations`

        // LE 偶尔需要 >30s 完成验证，把 retry 上限拉到 120s。
        let retry = RetryPolicy::new().timeout(Duration::from_secs(120));
        let status = order.poll_ready(&retry).await?;
        if !matches!(status, instant_acme::OrderStatus::Ready) {
            let detail = order
                .state()
                .error
                .as_ref()
                .map(|p| format!("{p:?}"))
                .unwrap_or_else(|| format!("status={status:?}"));
            return Err(AcmeError::OrderFailed(hostname.into(), detail));
        }

        // 本地生 ECDSA key + CSR；instant-acme 0.8 用 finalize_csr 区分自带 CSR 路径。
        let key_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
        let params = rcgen::CertificateParams::new(vec![hostname.to_string()])?;
        let csr = params.serialize_request(&key_pair)?;
        order.finalize_csr(csr.der().as_ref()).await?;

        let cert_chain_pem = order.poll_certificate(&retry).await?;

        // 解 notAfter 出 cert 第一段（leaf cert）
        let not_after_micros = parse_not_after_micros(&cert_chain_pem)
            .unwrap_or(chrono::Utc::now().timestamp_micros() + 90 * 24 * 3600 * 1_000_000);

        // key_pem 落 key_storage_dir/<hostname>.key.pem
        let key_pem = key_pair.serialize_pem();
        let key_path = self
            .inner
            .key_storage_dir
            .join(format!("{hostname}.key.pem"));
        tokio::fs::write(&key_path, &key_pem).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = tokio::fs::metadata(&key_path).await?.permissions();
            perms.set_mode(0o600);
            let _ = tokio::fs::set_permissions(&key_path, perms).await;
        }

        Ok(IssuedCert {
            hostname: hostname.to_string(),
            cert_pem: cert_chain_pem,
            key_pem,
            not_after_micros,
        })
    }
}

/// 提取 leaf cert 的 notAfter（unix micros）。
///
/// 用 `x509-cert` 解析 PEM 链的第一段（leaf），读其 `validity.notAfter` 并换成
/// unix micros。解析失败（非法 PEM / 无 cert / 时间溢出）返 `None`，由调用方
/// fallback 到保守的 90 天估算。修复前这里恒返 `None`，导致续期调度永远拿不到
/// 真实到期时间、存在证书静默过期风险。
fn parse_not_after_micros(cert_chain_pem: &str) -> Option<i64> {
    use x509_cert::der::Decode;

    // 先用 `pem` crate 解出第一个 CERTIFICATE block 的 DER（x509-cert 的 PEM 路径在
    // 非法输入上会 panic，这里用 pem 解析 + from_der 规避），leaf 是链的第一段。
    let blocks = pem::parse_many(cert_chain_pem).ok()?;
    let der = blocks.iter().find(|b| b.tag() == "CERTIFICATE")?.contents();
    let leaf = x509_cert::Certificate::from_der(der).ok()?;
    let micros = leaf
        .tbs_certificate
        .validity
        .not_after
        .to_unix_duration()
        .as_micros();
    i64::try_from(micros).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issued_cert_struct_round_trip() {
        let c = IssuedCert {
            hostname: "x.example.com".into(),
            cert_pem: "-----BEGIN CERT-----\nXXX\n-----END CERT-----\n".into(),
            key_pem: "-----BEGIN KEY-----\nYYY\n-----END KEY-----\n".into(),
            not_after_micros: 1_700_000_000_000_000,
        };
        assert_eq!(c.hostname, "x.example.com");
        assert!(c.cert_pem.contains("BEGIN CERT"));
    }

    #[test]
    fn parse_not_after_micros_falls_back_to_none() {
        assert!(parse_not_after_micros("garbage").is_none());
        assert!(parse_not_after_micros("").is_none());
    }

    #[test]
    fn parse_not_after_micros_reads_real_cert_validity() {
        // 生成一张 notAfter=2030-01-01 的自签证书，解析应精确到秒。
        let mut params = rcgen::CertificateParams::new(vec!["x.example.com".to_string()]).unwrap();
        params.not_after = rcgen::date_time_ymd(2030, 1, 1);
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = params.self_signed(&key).unwrap();
        let pem = cert.pem();

        let parsed = parse_not_after_micros(&pem).expect("leaf notAfter should parse");
        let expected = rcgen::date_time_ymd(2030, 1, 1).unix_timestamp() * 1_000_000;
        assert_eq!(parsed, expected);
    }
}
