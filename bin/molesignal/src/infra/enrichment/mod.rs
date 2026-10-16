// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MMDB / GeoIP enrichment（spec mmdb-enrichment）。
//!
//! [`geoip`]: `GeoIp` 实现 [`GeoLookup`]（`lookup(ip) -> Option<GeoLocation>`），背后 maxminddb
//! crate。库是 DB-IP City Lite（CC BY 4.0），不入库：容器镜像和二进制包在构建时下载当月版本，
//! 运行期不下载；RUM 写入经它按客户端 IP 填 `country`。
//!
//! VRL builtin `geoip_lookup(ip)` 在 VRL runtime 接入时绑（pipeline 里）。

pub mod geoip;

pub use geoip::{GeoIp, GeoLocation, GeoLookup};
