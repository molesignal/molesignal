// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 模型服务单价可选的币种：每个组织一份，管理员可以增改删。
//!
//! 代码是 ISO 4217 的三个大写字母，是单价与成本记账用的；名称与符号只管显示，为空时界面按读者
//! 的语言用标准的名称与符号。组织第一次读这份列表时写入常用的几种。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    agent::usage::Currency,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 组织第一次读时写入的币种，常用的在前。
pub const DEFAULT_CURRENCIES: [&str; 8] = ["USD", "CNY", "EUR", "GBP", "JPY", "HKD", "SGD", "KRW"];

/// 名称最多这么多个字符。
pub const MAX_NAME_CHARS: usize = 64;

/// 符号最多这么多个字符。
pub const MAX_SYMBOL_CHARS: usize = 8;

/// 组织备下的一种币种。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceCurrency {
    pub code: Currency,
    /// 自己起的名称；为空时用标准名称。
    pub name: Option<String>,
    /// 自己定的符号；为空时用标准符号。
    pub symbol: Option<String>,
    pub updated_at: TimestampMicros,
}

impl PriceCurrency {
    /// 校验并整理一份写入：首尾空白去掉，空串当作没有填。
    pub fn new(code: Currency, name: Option<&str>, symbol: Option<&str>) -> Result<Self> {
        Ok(Self {
            code,
            name: trimmed(name, "name", MAX_NAME_CHARS)?,
            symbol: trimmed(symbol, "symbol", MAX_SYMBOL_CHARS)?,
            updated_at: TimestampMicros::now(),
        })
    }
}

fn trimmed(value: Option<&str>, field: &str, max_chars: usize) -> Result<Option<String>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    if value.chars().count() > max_chars {
        return Err(Error::invalid(format!(
            "currency {field} must be at most {max_chars} characters"
        )));
    }

    Ok(Some(value.to_string()))
}

#[async_trait]
pub trait CurrencyRepository: Send + Sync {
    /// 组织的币种，先写入的在前；还一种都没有时先写入常用的几种。
    async fn list(&self, org_id: &Id) -> Result<Vec<PriceCurrency>>;

    /// 添加一种币种，或改它的名称与符号。
    async fn put(&self, org_id: &Id, currency: &PriceCurrency) -> Result<PriceCurrency>;

    /// 删掉一种币种；没有这种时返回 false。
    async fn delete(&self, org_id: &Id, code: Currency) -> Result<bool>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_symbols_are_trimmed_and_empty_means_standard() {
        let cny = Currency::parse("CNY").unwrap();
        let currency = PriceCurrency::new(cny, Some("  人民币 "), Some("")).unwrap();

        assert_eq!(currency.name.as_deref(), Some("人民币"));
        assert_eq!(currency.symbol, None);
        assert!(PriceCurrency::new(cny, None, Some("¥¥¥¥¥¥¥¥¥")).is_err());
        assert!(PriceCurrency::new(cny, Some(&"名".repeat(65)), None).is_err());
    }

    #[test]
    fn the_defaults_are_valid_codes() {
        for code in DEFAULT_CURRENCIES {
            assert!(Currency::parse(code).is_ok(), "{code}");
        }
    }
}
