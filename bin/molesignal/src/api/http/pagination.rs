// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 通用分页：`?page=&page_size=&filter=` extractor + `PageResponse<T>`。
//!
//! 不引入 `validator` crate；每个 *Request 自己实现 `Validate`（validate.rs）。

use serde::{Deserialize, Serialize};

pub mod cursor;

const DEFAULT_PAGE_SIZE: u32 = 50;
const MAX_PAGE_SIZE: u32 = 200;

#[derive(Debug, Clone, Deserialize)]
pub struct PageQuery {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    #[serde(default)]
    pub filter: Option<String>,
}

fn default_page() -> u32 {
    1
}
fn default_page_size() -> u32 {
    DEFAULT_PAGE_SIZE
}

impl PageQuery {
    pub fn clamp(&self) -> Self {
        Self {
            page: self.page.max(1),
            page_size: self.page_size.clamp(1, MAX_PAGE_SIZE),
            filter: self.filter.clone(),
        }
    }

    pub fn offset(&self) -> usize {
        let p = self.page.max(1) as usize;
        let s = self.page_size.clamp(1, MAX_PAGE_SIZE) as usize;
        (p - 1) * s
    }

    pub fn take(&self) -> usize {
        self.page_size.clamp(1, MAX_PAGE_SIZE) as usize
    }
}

impl Default for PageQuery {
    fn default() -> Self {
        Self {
            page: default_page(),
            page_size: default_page_size(),
            filter: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PageResponse<T> {
    pub items: Vec<T>,
    pub total: u64,
    pub page: u32,
    pub page_size: u32,
}

impl<T> PageResponse<T> {
    pub fn from_slice(items: Vec<T>, total: u64, q: &PageQuery) -> Self {
        let q = q.clamp();
        Self {
            items,
            total,
            page: q.page,
            page_size: q.page_size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_normalizes_bad_values() {
        let q = PageQuery {
            page: 0,
            page_size: 500,
            filter: None,
        };
        let c = q.clamp();
        assert_eq!(c.page, 1);
        assert_eq!(c.page_size, MAX_PAGE_SIZE);
    }

    #[test]
    fn offset_and_take_work() {
        let q = PageQuery {
            page: 3,
            page_size: 10,
            filter: None,
        };
        assert_eq!(q.offset(), 20);
        assert_eq!(q.take(), 10);
    }
}
