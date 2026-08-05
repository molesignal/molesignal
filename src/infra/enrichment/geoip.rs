// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! GeoIP lookup。
//!
//! `GeoIp::open(path)` 加载 MaxMind GeoLite2-City.mmdb；`lookup(ip) -> Option<GeoLocation>`。
//! 缺文件 / 解析失败 → `GeoIp::noop()` 永远返 None（不阻塞 ingest）。

use std::{net::IpAddr, path::Path, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoLocation {
    pub country: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
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

    pub fn lookup(&self, ip: IpAddr) -> Option<GeoLocation> {
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
    use super::*;

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
}
