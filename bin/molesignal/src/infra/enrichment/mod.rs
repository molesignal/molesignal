// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MMDB / GeoIP enrichment（spec mmdb-enrichment）。
//!
//! [`geoip`]: `GeoIp` 实现 [`GeoLookup`]（`lookup(ip) -> Option<GeoLocation>`），背后 maxminddb
//! crate。库随版本库（`assets/mmdb`）进入容器镜像和二进制包，运行期不下载；RUM 写入经它按
//! 客户端 IP 填 `country`。
//!
//! VRL builtin `geoip_lookup(ip)` 在 VRL runtime 接入时绑（pipeline 里）。

pub mod geoip;

pub use geoip::{GeoIp, GeoLocation, GeoLookup};
