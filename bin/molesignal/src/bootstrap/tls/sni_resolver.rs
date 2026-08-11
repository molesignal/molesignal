// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SNI-based cert resolver backed by `domains` 表 + 本地 key 文件（change `domain-acme-tls`）。
//!
//! 流程：
//! 1. `rustls::server::ResolvesServerCert::resolve(client_hello)` → 取 SNI hostname
//! 2. 内存 cache（DashMap, TTL 60s）命中直接返
//! 3. miss → `domains.find_by_hostname` + 读 `key_storage_dir/<hostname>.key.pem` →
//!    构 `rustls::sign::CertifiedKey` → 写 cache
//! 4. cert 更新后由 `acme_runner` 调 [`SniCertResolver::invalidate`] 失效该域

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::DashMap;
use rustls::{
    server::{ClientHello, ResolvesServerCert},
    sign::CertifiedKey,
};

use crate::infra::persistence::repositories::domains::DomainRepository;

const DEFAULT_TTL_SECS: u64 = 60;

#[derive(Clone)]
struct CachedEntry {
    key: Arc<CertifiedKey>,
    at: Instant,
}

pub struct SniCertResolver {
    domains: Arc<dyn DomainRepository>,
    cache: DashMap<String, CachedEntry>,
    ttl: Duration,
    key_dir: PathBuf,
    /// rustls 同步上下文里不能 `.await`；预热好的 tokio runtime handle 用于
    /// `block_on(find_by_hostname)`。caller 在创建 resolver 时传当前 runtime。
    runtime: tokio::runtime::Handle,
}

impl std::fmt::Debug for SniCertResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SniCertResolver")
            .field("cache_len", &self.cache.len())
            .field("ttl", &self.ttl)
            .field("key_dir", &self.key_dir)
            .finish()
    }
}

impl SniCertResolver {
    pub fn new(
        domains: Arc<dyn DomainRepository>,
        key_dir: PathBuf,
        runtime: tokio::runtime::Handle,
    ) -> Self {
        Self {
            domains,
            cache: DashMap::new(),
            ttl: Duration::from_secs(DEFAULT_TTL_SECS),
            key_dir,
            runtime,
        }
    }

    /// 测试钩子：覆盖 TTL。
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// 失效特定域的 cache（cert 更新后由 AcmeRunner 调）。
    pub fn invalidate(&self, hostname: &str) {
        self.cache.remove(hostname);
    }

    fn lookup(&self, hostname: &str) -> Option<Arc<CertifiedKey>> {
        if let Some(entry) = self.cache.get(hostname)
            && entry.at.elapsed() < self.ttl
        {
            return Some(entry.key.clone());
        }
        // miss / 过期 → 重读。rustls handshake 跑在普通 worker，但 tokio block_on
        // 在 worker 内部禁止；用 `block_in_place` + `Handle::block_on` 让 runtime
        // 把其他 task 临时搬走，再同步等查询。
        let domains = self.domains.clone();
        let handle = self.runtime.clone();
        let host = hostname.to_string();
        let row = tokio::task::block_in_place(|| {
            handle.block_on(async move { domains.find_by_hostname(&host).await })
        })
        .ok()
        .flatten()?;
        if row.state != "active" {
            return None;
        }
        let cert_pem = row.cert_pem?;
        let key_path = self.key_dir.join(format!("{hostname}.key.pem"));
        let key_pem = std::fs::read_to_string(&key_path).ok()?;

        let certified = build_certified_key(&cert_pem, &key_pem)?;
        let arc = Arc::new(certified);
        self.cache.insert(
            hostname.to_string(),
            CachedEntry {
                key: arc.clone(),
                at: Instant::now(),
            },
        );
        Some(arc)
    }
}

impl ResolvesServerCert for SniCertResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let sni = client_hello.server_name()?;
        self.lookup(sni)
    }
}

fn build_certified_key(cert_pem: &str, key_pem: &str) -> Option<CertifiedKey> {
    use std::io::BufReader;

    use rustls::crypto::CryptoProvider;
    use rustls_pemfile::Item;

    // parse cert chain
    let mut cert_reader = BufReader::new(cert_pem.as_bytes());
    let mut chain = Vec::new();
    for item in rustls_pemfile::read_all(&mut cert_reader).flatten() {
        if let Item::X509Certificate(c) = item {
            chain.push(c);
        }
    }
    if chain.is_empty() {
        return None;
    }

    // parse private key (first PKCS8 / RSA / SEC1)
    let mut key_reader = BufReader::new(key_pem.as_bytes());
    let key_der = rustls_pemfile::private_key(&mut key_reader)
        .ok()
        .flatten()?;

    let provider = CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(rustls_default_provider);
    let signing_key = provider.key_provider.load_private_key(key_der).ok()?;
    Some(CertifiedKey::new(chain, signing_key))
}

fn rustls_default_provider() -> Arc<rustls::crypto::CryptoProvider> {
    // aws-lc-rs is the default builtin; in test we may need to install it.
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::{
        infra::persistence::repositories::domains::{AcmeChallenge, DomainRow},
        shared::{Result as SharedResult, ids::Id, time::TimestampMicros},
    };

    /// Fake repo：只支持 find_by_hostname；其他 method panic。
    struct FakeRepo {
        row: Option<DomainRow>,
    }

    #[async_trait]
    impl DomainRepository for FakeRepo {
        async fn create(&self, _d: DomainRow) -> SharedResult<DomainRow> {
            unimplemented!()
        }
        async fn get(&self, _o: &Id, _i: &Id) -> SharedResult<DomainRow> {
            unimplemented!()
        }
        async fn find_by_hostname(&self, hostname: &str) -> SharedResult<Option<DomainRow>> {
            Ok(self
                .row
                .as_ref()
                .filter(|r| r.hostname == hostname)
                .cloned())
        }
        async fn list(&self, _o: &Id) -> SharedResult<Vec<DomainRow>> {
            unimplemented!()
        }
        async fn list_by_state(&self, _s: &str) -> SharedResult<Vec<DomainRow>> {
            unimplemented!()
        }
        async fn update_state(
            &self,
            _i: &Id,
            _s: &str,
            _c: Option<&str>,
            _n: Option<TimestampMicros>,
            _e: Option<&str>,
        ) -> SharedResult<()> {
            unimplemented!()
        }
        async fn delete(&self, _o: &Id, _i: &Id) -> SharedResult<()> {
            unimplemented!()
        }
        async fn put_challenge(&self, _c: AcmeChallenge) -> SharedResult<()> {
            unimplemented!()
        }
        async fn get_challenge(&self, _t: &str) -> SharedResult<Option<AcmeChallenge>> {
            unimplemented!()
        }
        async fn delete_expired_challenges(&self, _n: TimestampMicros) -> SharedResult<u64> {
            unimplemented!()
        }
    }

    fn rcgen_self_signed(hostname: &str) -> (String, String) {
        let pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let cert = rcgen::CertificateParams::new(vec![hostname.to_string()])
            .unwrap()
            .self_signed(&pair)
            .unwrap();
        (cert.pem(), pair.serialize_pem())
    }

    fn install_crypto_provider() {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unknown_sni_returns_none() {
        install_crypto_provider();
        let repo: Arc<dyn DomainRepository> = Arc::new(FakeRepo { row: None });
        let tmp = tempfile::tempdir().unwrap();
        let r = SniCertResolver::new(
            repo,
            tmp.path().to_path_buf(),
            tokio::runtime::Handle::current(),
        );
        assert!(r.lookup("unknown.example.com").is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn known_active_host_returns_certified_key() {
        install_crypto_provider();
        let host = "obs.example.com";
        let (cert_pem, key_pem) = rcgen_self_signed(host);
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(format!("{host}.key.pem")), &key_pem).unwrap();

        let row = DomainRow {
            id: Id::new(),
            org_id: Id("org-A".into()),
            hostname: host.into(),
            state: "active".into(),
            cert_pem: Some(cert_pem),
            cert_not_after: Some(TimestampMicros(1_900_000_000_000_000)),
            last_error: None,
            created_at: TimestampMicros::now(),
            updated_at: TimestampMicros::now(),
        };
        let repo: Arc<dyn DomainRepository> = Arc::new(FakeRepo { row: Some(row) });
        let r = SniCertResolver::new(
            repo,
            tmp.path().to_path_buf(),
            tokio::runtime::Handle::current(),
        );
        let key = r.lookup(host);
        assert!(key.is_some(), "expected CertifiedKey for active host");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn invalidate_clears_cache_entry() {
        install_crypto_provider();
        let host = "obs.example.com";
        let (cert_pem, key_pem) = rcgen_self_signed(host);
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(format!("{host}.key.pem")), &key_pem).unwrap();
        let row = DomainRow {
            id: Id::new(),
            org_id: Id("org-A".into()),
            hostname: host.into(),
            state: "active".into(),
            cert_pem: Some(cert_pem),
            cert_not_after: None,
            last_error: None,
            created_at: TimestampMicros::now(),
            updated_at: TimestampMicros::now(),
        };
        let repo: Arc<dyn DomainRepository> = Arc::new(FakeRepo { row: Some(row) });
        let r = SniCertResolver::new(
            repo,
            tmp.path().to_path_buf(),
            tokio::runtime::Handle::current(),
        );
        let _ = r.lookup(host);
        assert!(r.cache.contains_key(host));
        r.invalidate(host);
        assert!(!r.cache.contains_key(host));
    }
}
