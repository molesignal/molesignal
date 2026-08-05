// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Per-org 字段加密 DEK 解析 + 缓存（schema 驱动的字段级加密用）。
//!
//! - **写入**：[`FieldKeyService::current`] 返回该 org 的当前字段 DEK（首次自动 provision
//!   一把保留名 [`FIELD_DEFAULT_KEY_NAME`] 的 DEK）；ingester 用它加密 `encrypted` 字段。
//! - **查询**：[`FieldKeyService::decrypt_map`] 返回该 org 全部 DEK 的 `id→raw` 映射（含历史
//!   版本 + VRL 命名 key），供 `decrypt(col)` UDF 还原。
//!
//! DEK 行存于 `cipher_keys`（KEK 信封包），按 org 隔离；轮换 = 新增版本，旧版本仍可解历史。
//! 解析结果按 org 进 moka 缓存（短 TTL）：轮换 / provision 在一个 TTL 窗口内最终一致。

use std::{collections::HashMap, sync::Arc, time::Duration};

use moka::future::Cache;

use super::{super::keys::CipherKeyRepository, cipher::OrgFieldKey};
use crate::shared::{Error, Result, ids::Id};

/// 自动 provision 的默认字段加密 DEK 的保留 key 名。
pub const FIELD_DEFAULT_KEY_NAME: &str = "__field_default__";

/// 解析缓存的 TTL（秒）：轮换 / 首次 provision 在此窗口内对所有读者最终一致。
const CACHE_TTL_SECS: u64 = 60;

/// 单 org 的字段 DEK 视图。
struct OrgKeyset {
    /// 当前（最大版本）默认字段 DEK；org 尚未写过加密字段时为 `None`。
    default_current: Option<OrgFieldKey>,
    /// `key_id`(cipher_keys 行 id) → raw DEK，含该 org 全部 key 全部版本（解密用）。
    by_id: Arc<HashMap<String, Vec<u8>>>,
}

/// 字段加密 DEK 服务：包 [`CipherKeyRepository`] + per-org 缓存。
pub struct FieldKeyService {
    keys: Arc<dyn CipherKeyRepository>,
    cache: Cache<String, Arc<OrgKeyset>>,
}

impl FieldKeyService {
    pub fn new(keys: Arc<dyn CipherKeyRepository>) -> Self {
        Self {
            keys,
            cache: Cache::builder()
                .max_capacity(4096)
                .time_to_live(Duration::from_secs(CACHE_TTL_SECS))
                .build(),
        }
    }

    async fn load(&self, org: &Id) -> Result<Arc<OrgKeyset>> {
        if let Some(k) = self.cache.get(&org.0).await {
            return Ok(k);
        }
        // `list` 按 (name, version DESC) 排序：每个 name 的第一行即最大版本。
        let all = self.keys.list(org).await?;
        let mut by_id: HashMap<String, Vec<u8>> = HashMap::with_capacity(all.len());
        let mut default_current: Option<OrgFieldKey> = None;
        for k in all {
            if k.name == FIELD_DEFAULT_KEY_NAME && default_current.is_none() {
                default_current = Some(OrgFieldKey {
                    key_id: k.id.0.clone(),
                    version: k.version,
                    raw_key: k.raw_key.clone(),
                });
            }
            by_id.insert(k.id.0, k.raw_key);
        }
        let arc = Arc::new(OrgKeyset {
            default_current,
            by_id: Arc::new(by_id),
        });
        self.cache.insert(org.0.clone(), arc.clone()).await;
        Ok(arc)
    }

    /// 该 org 的当前字段加密 DEK；不存在则自动 provision 一把（保留名 DEK）。
    pub async fn current(&self, org: &Id) -> Result<OrgFieldKey> {
        if let Some(c) = self.load(org).await?.default_current.clone() {
            return Ok(c);
        }
        // provision：生成 32B → create。并发首写竞态 / 已存在 → 忽略错误后重读。
        let raw = random_key()?;
        if let Err(e) = self.keys.create(org, FIELD_DEFAULT_KEY_NAME, &raw).await {
            tracing::debug!(org = %org.0, error = %e, "field DEK create raced or failed; reloading");
        }
        self.cache.invalidate(&org.0).await;
        self.load(org)
            .await?
            .default_current
            .clone()
            .ok_or_else(|| Error::internal("provision field DEK failed"))
    }

    /// 该 org 全部 DEK 的 `id→raw` 映射（解密用，含历史版本）。
    pub async fn decrypt_map(&self, org: &Id) -> Result<Arc<HashMap<String, Vec<u8>>>> {
        Ok(self.load(org).await?.by_id.clone())
    }

    /// 失效该 org 的缓存：cipher_keys 发生增删 / 轮换后调用，使下次 `current` / `decrypt_map`
    /// 立刻重读，消除「轮换后新版本数据在 TTL 窗口内不可解」的间隙（本进程内即时生效；
    /// 多进程部署其它节点仍受 TTL 兜底）。
    pub async fn invalidate(&self, org: &Id) {
        self.cache.invalidate(&org.0).await;
    }

    /// 轮换该 org 的字段加密 DEK（服务端生成新 raw key，写入新版本，失效缓存）。
    /// 新写入用新版本；历史密文仍可解（`decrypt_map` 含全版本）。
    pub async fn rotate_default(&self, org: &Id) -> Result<OrgFieldKey> {
        let raw = random_key()?;
        let k = self.keys.rotate(org, FIELD_DEFAULT_KEY_NAME, &raw).await?;
        self.cache.invalidate(&org.0).await;
        Ok(OrgFieldKey {
            key_id: k.id.0,
            version: k.version,
            raw_key: k.raw_key,
        })
    }
}

fn random_key() -> Result<Vec<u8>> {
    use rand::TryRng as _;
    let mut buf = [0u8; 32];
    rand::rngs::SysRng
        .try_fill_bytes(&mut buf)
        .map_err(|e| Error::internal(format!("rng: {e}")))?;
    Ok(buf.to_vec())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use async_trait::async_trait;

    use super::{super::super::keys::CipherKey, *};
    use crate::shared::time::TimestampMicros;

    /// 进程内 CipherKeyRepository：raw key 直存（不经 KEK），仅测试用。
    #[derive(Default)]
    struct InMemKeys {
        rows: StdMutex<Vec<CipherKey>>,
    }

    #[async_trait]
    impl CipherKeyRepository for InMemKeys {
        async fn create(&self, org_id: &Id, name: &str, raw_key: &[u8]) -> Result<CipherKey> {
            let mut rows = self.rows.lock().unwrap();
            if rows.iter().any(|r| &r.org_id == org_id && r.name == name) {
                return Err(Error::invalid("duplicate"));
            }
            let k = CipherKey {
                id: Id::new(),
                org_id: org_id.clone(),
                name: name.to_string(),
                alg: "aes-256-gcm".into(),
                version: 1,
                raw_key: raw_key.to_vec(),
                created_at: TimestampMicros(0),
                rotated_at: None,
            };
            rows.push(k.clone());
            Ok(k)
        }
        async fn rotate(&self, org_id: &Id, name: &str, raw_key: &[u8]) -> Result<CipherKey> {
            let mut rows = self.rows.lock().unwrap();
            let next = rows
                .iter()
                .filter(|r| &r.org_id == org_id && r.name == name)
                .map(|r| r.version)
                .max()
                .unwrap_or(0)
                + 1;
            let k = CipherKey {
                id: Id::new(),
                org_id: org_id.clone(),
                name: name.to_string(),
                alg: "aes-256-gcm".into(),
                version: next,
                raw_key: raw_key.to_vec(),
                created_at: TimestampMicros(0),
                rotated_at: Some(TimestampMicros(0)),
            };
            rows.push(k.clone());
            Ok(k)
        }
        async fn get_latest(&self, org_id: &Id, name: &str) -> Result<CipherKey> {
            self.rows
                .lock()
                .unwrap()
                .iter()
                .filter(|r| &r.org_id == org_id && r.name == name)
                .max_by_key(|r| r.version)
                .cloned()
                .ok_or_else(|| Error::not_found("key"))
        }
        async fn get_by_id_version(
            &self,
            org_id: &Id,
            key_id: &str,
            version: i32,
        ) -> Result<CipherKey> {
            self.rows
                .lock()
                .unwrap()
                .iter()
                .find(|r| &r.org_id == org_id && r.id.0 == key_id && r.version == version)
                .cloned()
                .ok_or_else(|| Error::not_found("key"))
        }
        async fn list(&self, org_id: &Id) -> Result<Vec<CipherKey>> {
            let mut v: Vec<CipherKey> = self
                .rows
                .lock()
                .unwrap()
                .iter()
                .filter(|r| &r.org_id == org_id)
                .cloned()
                .collect();
            // 模拟 PG 的 ORDER BY name, version DESC。
            v.sort_by(|a, b| a.name.cmp(&b.name).then(b.version.cmp(&a.version)));
            Ok(v)
        }
        async fn delete(&self, org_id: &Id, name: &str) -> Result<()> {
            self.rows
                .lock()
                .unwrap()
                .retain(|r| !(&r.org_id == org_id && r.name == name));
            Ok(())
        }
    }

    #[tokio::test]
    async fn current_auto_provisions_default_dek() {
        let repo = Arc::new(InMemKeys::default());
        let svc = FieldKeyService::new(repo.clone());
        let org = Id::from_string("org-1");

        let k1 = svc.current(&org).await.unwrap();
        assert_eq!(k1.version, 1);
        assert_eq!(k1.raw_key.len(), 32);
        // provision 后 DB 里有一把保留名 DEK。
        assert_eq!(repo.list(&org).await.unwrap().len(), 1);
        assert_eq!(
            repo.get_latest(&org, FIELD_DEFAULT_KEY_NAME)
                .await
                .unwrap()
                .version,
            1
        );
    }

    #[tokio::test]
    async fn decrypt_map_covers_all_versions_after_rotation() {
        let repo = Arc::new(InMemKeys::default());
        let svc = FieldKeyService::new(repo.clone());
        let org = Id::from_string("org-1");
        let v1 = svc.current(&org).await.unwrap();
        // 轮换：新增 v2（绕过缓存直接动 repo），decrypt_map 应含两个版本的 id。
        let v2 = repo
            .rotate(&org, FIELD_DEFAULT_KEY_NAME, &[9u8; 32])
            .await
            .unwrap();
        svc.cache.invalidate(&org.0).await; // 模拟 TTL 过期
        let map = svc.decrypt_map(&org).await.unwrap();
        assert!(map.contains_key(&v1.key_id), "old version retained");
        assert!(map.contains_key(&v2.id.0), "new version present");
    }

    #[tokio::test]
    async fn rotate_default_bumps_version_and_refreshes_immediately() {
        let repo = Arc::new(InMemKeys::default());
        let svc = FieldKeyService::new(repo.clone());
        let org = Id::from_string("org-1");

        let v1 = svc.current(&org).await.unwrap();
        assert_eq!(v1.version, 1);

        // 轮换：服务端生成新 key，版本 +1，并即时失效缓存。
        let v2 = svc.rotate_default(&org).await.unwrap();
        assert_eq!(v2.version, 2);
        assert_ne!(v2.raw_key, v1.raw_key, "fresh key material");

        // current 立刻返回新版本（无 TTL 窗口）。
        assert_eq!(svc.current(&org).await.unwrap().version, 2);
        // decrypt_map 含新旧两版本 → 轮换后写入的新数据与历史都可解。
        let map = svc.decrypt_map(&org).await.unwrap();
        assert!(
            map.contains_key(&v1.key_id),
            "old version still decryptable"
        );
        assert!(map.contains_key(&v2.key_id), "new version decryptable");
    }
}
