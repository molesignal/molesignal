// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! GeoIP lookup。
//!
//! `GeoIp::open(path)` 加载 MMDB 格式的城市库（DB-IP City Lite，构建时由
//! `scripts/fetch-dbip-city-lite.sh` 下载）；`lookup(ip) -> Option<GeoLocation>`。
//! 缺文件 / 解析失败 → `GeoIp::noop()` 永远返 None（不阻塞 intake）。
//!
//! 写入路径经 [`GeoLookup`] 查找，测试可换成假查找器。

use std::{net::IpAddr, path::Path, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeoLocation {
    pub country: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

/// 按 IP 查地理位置；查不到时为 None。
pub trait GeoLookup: Send + Sync {
    fn lookup(&self, ip: IpAddr) -> Option<GeoLocation>;
}

enum Inner {
    Db(maxminddb::Reader<Vec<u8>>),
    Noop,
}

pub struct GeoIp {
    inner: Inner,
}

impl GeoIp {
    pub fn open(path: &Path) -> Result<Arc<Self>> {
        let reader = maxminddb::Reader::open_readfile(path)
            .map_err(|e| Error::internal(format!("mmdb open {}: {e}", path.display())))?;

        Ok(Arc::new(Self {
            inner: Inner::Db(reader),
        }))
    }

    pub fn noop() -> Arc<Self> {
        Arc::new(Self { inner: Inner::Noop })
    }

    /// 打开 `path`；文件缺失或解析失败时退回空库，写入照常进行。
    pub fn open_or_noop(path: &Path) -> Arc<Self> {
        match Self::open(path) {
            Ok(geoip) => {
                tracing::info!(path = %path.display(), "GeoIP database loaded");
                geoip
            }
            Err(error) => {
                tracing::warn!(%error, "GeoIP database unavailable; lookups find nothing");
                Self::noop()
            }
        }
    }
}

impl GeoLookup for GeoIp {
    fn lookup(&self, ip: IpAddr) -> Option<GeoLocation> {
        let reader = match &self.inner {
            Inner::Db(r) => r,
            Inner::Noop => return None,
        };

        // maxminddb 0.27：reader.lookup(ip) → Result<LookupResult, Error>；
        // LookupResult::decode::<T>() 返 `Result<Option<T>, Error>`。
        let lookup = reader.lookup(ip).ok()?;
        let city: maxminddb::geoip2::City = lookup.decode().ok()??;

        Some(GeoLocation {
            country: city.country.iso_code.map(String::from),
            region: city
                .subdivisions
                .first()
                .and_then(|s| s.names.english.map(String::from)),
            city: city.city.names.english.map(String::from),
            latitude: city.location.latitude,
            longitude: city.location.longitude,
        })
    }
}

/// 字符串 IP → IpAddr，便于 VRL 入参。
pub fn parse_ip(s: &str) -> Option<IpAddr> {
    s.parse().ok()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// DB-IP City Lite 不入库：`make geoip-db` 把它下载到 `assets/mmdb/`，`MS_TEST_MMDB` 可指向
    /// 别处的文件。
    fn downloaded_mmdb() -> PathBuf {
        std::env::var_os("MS_TEST_MMDB")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../assets/mmdb/dbip-city-lite.mmdb")
            })
    }

    fn scratch_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "molesignal-geoip-{name}-{}-{}.mmdb",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ))
    }

    fn country(geoip: &GeoIp, ip: &str) -> Option<String> {
        geoip.lookup(ip.parse().unwrap())?.country
    }

    #[test]
    fn noop_lookup_returns_none() {
        let g = GeoIp::noop();
        assert!(g.lookup("8.8.8.8".parse().unwrap()).is_none());
    }

    #[test]
    fn parse_ip_handles_ipv4_and_ipv6() {
        assert!(parse_ip("192.168.1.1").is_some());
        assert!(parse_ip("2001:db8::1").is_some());
        assert!(parse_ip("not-an-ip").is_none());
    }

    #[test]
    #[ignore = "needs the DB-IP City Lite file: make geoip-db"]
    fn city_lite_resolves_public_addresses_only() {
        let geoip = GeoIp::open(&downloaded_mmdb()).unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(country(&geoip, "8.8.8.8").as_deref(), Some("US"));
        assert_eq!(country(&geoip, "10.0.0.1"), None);
    }

    #[test]
    fn missing_or_unreadable_database_falls_back_to_noop() {
        let missing = GeoIp::open_or_noop(&scratch_file("missing"));
        assert_eq!(country(&missing, "8.8.8.8"), None);

        let garbage = scratch_file("garbage");
        std::fs::write(&garbage, b"not an mmdb").unwrap();
        let unreadable = GeoIp::open_or_noop(&garbage);
        let _ = std::fs::remove_file(&garbage);

        assert_eq!(country(&unreadable, "8.8.8.8"), None);
    }
}
