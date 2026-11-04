// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! IP 地址的可比较形式：`ip` 索引的字段按它记下段里地址的范围，`cidr_match` 按它判断网段。
//!
//! IPv4 地址映射进 IPv6（`::ffff:a.b.c.d`），两种地址落在同一条数轴上，`10.0.0.0/8` 与
//! `::ffff:10.0.0.0/104` 是同一段。

use std::net::IpAddr;

/// 地址的 128 位数值，IPv4 映射进 IPv6。不是地址（含端口、作用域、空白）时为 `None`。
pub fn address_number(text: &str) -> Option<u128> {
    Some(match text.parse::<IpAddr>().ok()? {
        IpAddr::V4(address) => u128::from(address.to_ipv6_mapped()),
        IpAddr::V6(address) => u128::from(address),
    })
}

/// 网段（`10.0.0.0/8`、`2001:db8::/32`，或不带前缀长度的单个地址）覆盖的数值范围，含两端。
/// 前缀长度超出地址位数、或写法不对时为 `None`。
pub fn cidr_range(text: &str) -> Option<(u128, u128)> {
    let (address, prefix) = match text.split_once('/') {
        Some((address, prefix)) => (address, Some(prefix.parse::<u32>().ok()?)),
        None => (text, None),
    };

    let (number, width) = match address.parse::<IpAddr>().ok()? {
        IpAddr::V4(address) => (u128::from(address.to_ipv6_mapped()), 32),
        IpAddr::V6(address) => (u128::from(address), 128),
    };

    let prefix = prefix.unwrap_or(width);

    if prefix > width {
        return None;
    }

    // IPv4 的前缀落在映射前缀 `::ffff:0:0/96` 之后。
    let bits = 128 - width + prefix;

    let mask = u128::MAX.checked_shl(128 - bits).unwrap_or(0);
    let low = number & mask;

    Some((low, low | !mask))
}

/// 32 位小写十六进制：等长，字符串的先后就是数的先后。
pub fn address_key(number: u128) -> String {
    format!("{number:032x}")
}

/// 读回 [`address_key`]。
pub fn parse_address_key(key: &str) -> Option<u128> {
    (key.len() == 32)
        .then(|| u128::from_str_radix(key, 16).ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_lands_inside_the_mapped_ipv6_range() {
        let ten = address_number("10.1.2.3").unwrap();
        let (low, high) = cidr_range("10.0.0.0/8").unwrap();

        assert!(low <= ten && ten <= high);
        assert_eq!(address_number("::ffff:10.1.2.3"), Some(ten));
        assert_eq!(cidr_range("::ffff:10.0.0.0/104"), Some((low, high)));
        assert!(!(low..=high).contains(&address_number("11.0.0.1").unwrap()));
    }

    #[test]
    fn whole_and_single_address_ranges() {
        assert_eq!(cidr_range("::/0"), Some((0, u128::MAX)));

        let one = address_number("192.168.0.1").unwrap();
        assert_eq!(cidr_range("192.168.0.1"), Some((one, one)));
        assert_eq!(cidr_range("192.168.0.1/32"), Some((one, one)));
    }

    #[test]
    fn malformed_text_is_not_an_address() {
        for text in ["10.0.0.1:80", " 10.0.0.1", "fe80::1%eth0", "host", ""] {
            assert_eq!(address_number(text), None, "{text}");
        }

        for text in ["10.0.0.0/33", "::/129", "10.0.0.0/x", "10.0.0/8"] {
            assert_eq!(cidr_range(text), None, "{text}");
        }
    }

    #[test]
    fn keys_sort_like_the_numbers() {
        let low = address_key(address_number("9.255.255.255").unwrap());
        let high = address_key(address_number("10.0.0.0").unwrap());

        assert!(low < high);
        assert_eq!(parse_address_key(&high), address_number("10.0.0.0"));
        assert_eq!(parse_address_key("ff"), None);
    }
}
