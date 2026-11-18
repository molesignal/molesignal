// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Currencies available for model service unit prices, maintained per organization, which
//! administrators can add, edit, and delete.
//!
//! The code consists of three uppercase ISO 4217 letters and is used for unit prices and cost
//! accounting. The name and symbol are for display only; when empty, the UI uses the standard name
//! and symbol in the reader's language. Common currencies are added when the organization first
//! reads this list.

use async_trait::async_trait;
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};

use crate::usage::Currency;

/// Currencies added on the organization's first read, with common ones first.
pub const DEFAULT_CURRENCIES: [&str; 8] = ["USD", "CNY", "EUR", "GBP", "JPY", "HKD", "SGD", "KRW"];

/// The maximum number of characters in a name.
pub const MAX_NAME_CHARS: usize = 64;

/// The maximum number of characters in a symbol.
pub const MAX_SYMBOL_CHARS: usize = 8;

/// A currency configured by an organization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceCurrency {
    pub code: Currency,
    /// A custom name; uses the standard name when empty.
    pub name: Option<String>,
    /// A custom symbol; uses the standard symbol when empty.
    pub symbol: Option<String>,
    pub updated_at: TimestampMicros,
}

impl PriceCurrency {
    /// Validates and normalizes a write: trims whitespace and treats empty strings as unspecified.
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
    /// The organization's currencies, earliest additions first; adds common ones if none exist yet.
    async fn list(&self, org_id: &Id) -> Result<Vec<PriceCurrency>>;

    /// Adds a currency or changes its name and symbol.
    async fn put(&self, org_id: &Id, currency: &PriceCurrency) -> Result<PriceCurrency>;

    /// Deletes a currency; returns false if it does not exist.
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
