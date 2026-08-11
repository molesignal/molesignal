// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MaxMind GeoLite2 mmdb 下载器。
//!
//! - 启动期优先使用发布包内置的 `GeoLite2-City.mmdb`。
//! - 若本地 db 不存在且 `license_key` 已设 → 异步 HTTP 下载 tar.gz + 提取到 `db_path`。
//! - 周期 refresh 只有配置 license key 时启用；缺 key 仅禁用刷新，不阻塞启动。
//!
//! 下载实现：`reqwest` 拉 tar.gz → `flate2 + tar` 提取 → 写文件原子替换。
//! 当前取消文件 atomic rename 兜底（写临时 + rename），简化为直写。

use std::{path::PathBuf, sync::Arc, time::Duration};

use crate::shared::{Error, Result};

#[derive(Debug, Clone)]
pub struct MmdbConfig {
    pub license_key: Option<String>,
    pub db_path: PathBuf,
    pub refresh_interval_secs: u64,
}

impl Default for MmdbConfig {
    fn default() -> Self {
        Self {
            license_key: None,
            db_path: PathBuf::from("/usr/share/molesignal/mmdb/GeoLite2-City.mmdb"),
            refresh_interval_secs: 7 * 24 * 3600,
        }
    }
}

pub struct MmdbDownloader {
    cfg: MmdbConfig,
}

impl MmdbDownloader {
    pub fn new(cfg: MmdbConfig) -> Arc<Self> {
        Arc::new(Self { cfg })
    }

    /// 启动时调用：优先使用内置/本地 db；缺文件且有 key 时拉一次。
    /// 返回 true 表示文件就位、false 表示未就位（caller 决定是否 fail-soft）。
    pub async fn ensure_ready(&self) -> Result<bool> {
        if self.cfg.db_path.exists() {
            tracing::info!(path = %self.cfg.db_path.display(), "mmdb file present");
            return Ok(true);
        }
        if self.cfg.license_key.is_none() {
            tracing::warn!(
                path = %self.cfg.db_path.display(),
                "MMDB file not found and license_key not configured; geoip_lookup will return null for all IPs"
            );
            return Ok(false);
        }
        match self.download_once().await {
            Ok(()) => {
                tracing::info!(path = %self.cfg.db_path.display(), "mmdb downloaded");
                Ok(true)
            }
            Err(e) => {
                tracing::warn!(error = %e, "mmdb download failed; place file manually");
                Ok(false)
            }
        }
    }

    /// 真正的下载：HTTP GET tarball → gunzip + 走 tar entries → 写出 `GeoLite2-City.mmdb`。
    async fn download_once(&self) -> Result<()> {
        let key = self
            .cfg
            .license_key
            .as_ref()
            .ok_or_else(|| Error::invalid("mmdb license_key not set"))?;
        let url = format!(
            "https://download.maxmind.com/app/geoip_download\
             ?edition_id=GeoLite2-City&suffix=tar.gz&license_key={key}"
        );
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| Error::internal(format!("reqwest build: {e}")))?;
        let resp = crate::shared::http_trace::send(
            &client,
            client.get(&url),
            crate::shared::http_trace::HttpTarget::ThirdParty,
        )
        .await
        .map_err(|e| Error::internal(format!("mmdb HTTP get: {e}")))?;
        if !resp.status().is_success() {
            return Err(Error::internal(format!(
                "mmdb HTTP {} from MaxMind",
                resp.status()
            )));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| Error::internal(format!("mmdb read body: {e}")))?;
        let bytes = bytes.to_vec();

        // tar.gz 提取在 spawn_blocking 里跑（同步 IO + 解压）
        let target = self.cfg.db_path.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| Error::internal(format!("mkdir mmdb parent: {e}")))?;
            }
            let cursor = std::io::Cursor::new(bytes);
            let gz = flate2::read::GzDecoder::new(cursor);
            let mut ar = tar::Archive::new(gz);
            for entry in ar
                .entries()
                .map_err(|e| Error::internal(format!("tar entries: {e}")))?
            {
                let mut entry = entry.map_err(|e| Error::internal(format!("tar entry: {e}")))?;
                let path = entry
                    .path()
                    .map_err(|e| Error::internal(format!("tar path: {e}")))?
                    .into_owned();
                if path
                    .file_name()
                    .map(|n| n == "GeoLite2-City.mmdb")
                    .unwrap_or(false)
                {
                    let tmp = target.with_extension("mmdb.tmp");
                    let mut f = std::fs::File::create(&tmp)
                        .map_err(|e| Error::internal(format!("create tmp: {e}")))?;
                    std::io::copy(&mut entry, &mut f)
                        .map_err(|e| Error::internal(format!("write tmp: {e}")))?;
                    std::fs::rename(&tmp, &target)
                        .map_err(|e| Error::internal(format!("rename mmdb: {e}")))?;
                    return Ok(());
                }
            }
            Err(Error::internal(
                "GeoLite2-City.mmdb entry not found in tarball",
            ))
        })
        .await
        .map_err(|e| Error::internal(format!("spawn_blocking: {e}")))??;
        Ok(())
    }

    /// 周期 refresh：每 `refresh_interval_secs` 拉一次（失败 warn 不停）。
    pub fn spawn_refresh(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        if self.cfg.license_key.is_none() {
            let path = self.cfg.db_path.clone();
            return tokio::spawn(async move {
                tracing::info!(
                    path = %path.display(),
                    "MMDB license_key not configured; periodic refresh disabled"
                );
                std::future::pending::<()>().await;
            });
        }

        let secs = self.cfg.refresh_interval_secs.max(3600);
        let me = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(secs));
            ticker.tick().await; // 首 tick 跳过（startup ensure_ready 已经拉过）
            loop {
                ticker.tick().await;
                if let Err(e) = me.download_once().await {
                    tracing::warn!(error = %e, "mmdb refresh failed");
                }
            }
        })
    }

    pub fn db_path(&self) -> &std::path::Path {
        &self.cfg.db_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_key_returns_false() {
        let d = MmdbDownloader::new(MmdbConfig {
            license_key: None,
            db_path: PathBuf::from("/tmp/nonexistent-test.mmdb"),
            refresh_interval_secs: 0,
        });
        assert!(!d.ensure_ready().await.unwrap());
    }

    #[tokio::test]
    async fn existing_file_without_key_is_ready() {
        let path = std::env::temp_dir().join(format!(
            "molesignal-mmdb-existing-{}-{}.mmdb",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::write(&path, b"test-mmdb").unwrap();

        let d = MmdbDownloader::new(MmdbConfig {
            license_key: None,
            db_path: path.clone(),
            refresh_interval_secs: 0,
        });
        assert!(d.ensure_ready().await.unwrap());

        let _ = std::fs::remove_file(path);
    }
}
