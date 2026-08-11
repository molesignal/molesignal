// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MMDB / GeoIP enrichment（spec mmdb-enrichment）。
//!
//! - [`geoip`]: `GeoIp` 单例 + `lookup(ip) -> Option<GeoLocation>`，背后 maxminddb crate
//! - [`mmdb_downloader`]: 启动期下载 + 周 cron refresh（当前仅占位 stub）
//!
//! VRL builtin `geoip_lookup(ip)` 在 VRL runtime 接入时绑（pipeline 里）。

pub mod geoip;
pub mod mmdb_downloader;

pub use geoip::{GeoIp, GeoLocation};
pub use mmdb_downloader::MmdbDownloader;
