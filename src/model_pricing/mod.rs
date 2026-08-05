// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! LLM model pricing catalog（spec model-pricing，付费版独占）。
//!
//! 提供：
//! - **PricingCatalog** —— 模型 → (input/output per-million USD)；按 `effective_at`
//!   取最新生效价
//! - **default_seed()** —— 启动 seed 当前主流模型的默认价格
//! - **compute_cost(model, prompt_tok, completion_tok)** —— 计算单次调用成本

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::shared::{Result, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPrice {
    pub provider: String, // openai / anthropic / etc.
    pub model: String,    // gpt-4o, claude-3.5-sonnet, ...
    pub input_per_million_usd: f64,
    pub output_per_million_usd: f64,
    pub effective_at: TimestampMicros,
}

#[async_trait]
pub trait ModelPriceRepository: Send + Sync {
    async fn upsert(&self, p: ModelPrice) -> Result<ModelPrice>;
    async fn list(&self) -> Result<Vec<ModelPrice>>;
    async fn delete(&self, provider: &str, model: &str) -> Result<()>;
}

/// 进程内 catalog：从 repo 启动期一次性加载，handler 走 catalog 不走 DB。
pub struct PricingCatalog {
    inner: Arc<RwLock<HashMap<(String, String), ModelPrice>>>,
}

impl Default for PricingCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl PricingCatalog {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 启动时填充默认价格表（迁移后 seed）。
    pub fn with_defaults() -> Self {
        let s = Self::new();
        for p in default_seed() {
            s.upsert(p);
        }
        s
    }

    pub fn upsert(&self, p: ModelPrice) {
        let mut g = self.inner.write();
        g.insert((p.provider.clone(), p.model.clone()), p);
    }

    pub fn get(&self, provider: &str, model: &str) -> Option<ModelPrice> {
        let g = self.inner.read();
        g.get(&(provider.to_string(), model.to_string())).cloned()
    }

    pub fn list(&self) -> Vec<ModelPrice> {
        let g = self.inner.read();
        g.values().cloned().collect()
    }

    pub fn size(&self) -> usize {
        self.inner.read().len()
    }
}

/// 默认价格表（spec seed）。价位参考 2024-Q4 公开价；可由后续迁移覆盖。
pub fn default_seed() -> Vec<ModelPrice> {
    let now = TimestampMicros::now();
    vec![
        ModelPrice {
            provider: "openai".into(),
            model: "gpt-4o".into(),
            input_per_million_usd: 5.0,
            output_per_million_usd: 15.0,
            effective_at: now,
        },
        ModelPrice {
            provider: "openai".into(),
            model: "gpt-4o-mini".into(),
            input_per_million_usd: 0.15,
            output_per_million_usd: 0.60,
            effective_at: now,
        },
        ModelPrice {
            provider: "anthropic".into(),
            model: "claude-3-5-sonnet".into(),
            input_per_million_usd: 3.0,
            output_per_million_usd: 15.0,
            effective_at: now,
        },
        ModelPrice {
            provider: "anthropic".into(),
            model: "claude-3-haiku".into(),
            input_per_million_usd: 0.25,
            output_per_million_usd: 1.25,
            effective_at: now,
        },
    ]
}

/// 计算单次 LLM 调用成本（USD）。模型缺失时返 0.0。
pub fn compute_cost(
    catalog: &PricingCatalog,
    provider: &str,
    model: &str,
    prompt_tokens: i32,
    completion_tokens: i32,
) -> f64 {
    let p = match catalog.get(provider, model) {
        Some(p) => p,
        None => return 0.0,
    };
    let prompt = prompt_tokens.max(0) as f64;
    let completion = completion_tokens.max(0) as f64;
    (prompt * p.input_per_million_usd + completion * p.output_per_million_usd) / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_seed_contains_known_models() {
        let c = PricingCatalog::with_defaults();
        assert!(c.get("openai", "gpt-4o").is_some());
        assert!(c.get("openai", "gpt-4o-mini").is_some());
        assert!(c.get("anthropic", "claude-3-5-sonnet").is_some());
        assert!(c.get("anthropic", "claude-3-haiku").is_some());
        assert_eq!(c.size(), 4);
    }

    #[test]
    fn compute_cost_matches_spec_example() {
        // gpt-4o 1000 prompt + 500 completion 应是 0.0125 USD
        let c = PricingCatalog::with_defaults();
        let cost = compute_cost(&c, "openai", "gpt-4o", 1000, 500);
        assert!((cost - 0.0125).abs() < 1e-9, "got {cost}");
    }

    #[test]
    fn missing_model_returns_zero() {
        let c = PricingCatalog::new(); // 空
        let cost = compute_cost(&c, "openai", "gpt-4o", 1000, 500);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn upsert_replaces_existing() {
        let c = PricingCatalog::with_defaults();
        c.upsert(ModelPrice {
            provider: "openai".into(),
            model: "gpt-4o".into(),
            input_per_million_usd: 10.0,
            output_per_million_usd: 30.0,
            effective_at: TimestampMicros::now(),
        });
        let p = c.get("openai", "gpt-4o").unwrap();
        assert_eq!(p.input_per_million_usd, 10.0);
        assert_eq!(p.output_per_million_usd, 30.0);
    }
}
