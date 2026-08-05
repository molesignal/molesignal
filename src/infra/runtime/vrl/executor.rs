// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `FunctionExecutor` 的 VRL 实装。
//!
//! 按 `function.id` 缓存 `CompiledProgram`，避免每条 event 重新编译。
//! 缓存键还带 `function.updated_at` 微秒戳，源码改了缓存自动失效。

use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;

use crate::{
    app::ingestion::FunctionExecutor,
    domain::function::{Function, FunctionLanguage},
    infra::runtime::vrl::runtime::{CompiledProgram, VrlRuntime},
    shared::{Error, Result},
};

pub struct VrlFunctionExecutor {
    runtime: VrlRuntime,
    /// `(function_id, updated_at_micros)` → 已编译程序。
    cache: DashMap<(String, i64), Arc<CompiledProgram>>,
}

impl VrlFunctionExecutor {
    pub fn new() -> Self {
        Self {
            runtime: VrlRuntime::new(),
            cache: DashMap::new(),
        }
    }

    fn compile_cached(&self, f: &Function) -> Result<Arc<CompiledProgram>> {
        let key = (f.id.0.clone(), f.updated_at.0);
        if let Some(hit) = self.cache.get(&key) {
            return Ok(hit.clone());
        }
        let compiled = Arc::new(self.runtime.compile(&f.source)?);
        self.cache.insert(key, compiled.clone());
        // 清理同 function_id 的旧版本，避免缓存膨胀
        self.cache
            .retain(|k, _| k.0 != f.id.0 || k.1 == f.updated_at.0);
        Ok(compiled)
    }
}

impl Default for VrlFunctionExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FunctionExecutor for VrlFunctionExecutor {
    async fn run(&self, function: &Function, event: &mut serde_json::Value) -> Result<()> {
        match function.language {
            FunctionLanguage::Vrl => {
                let prog = self.compile_cached(function)?;
                self.runtime.run(&prog, event)
            }
            FunctionLanguage::Js => Err(Error::invalid(
                "javascript runtime not yet implemented (build with feature=js)",
            )),
            FunctionLanguage::Llm => Err(Error::invalid(
                "LLM eval routed to VRL executor (should go through the chained executor)",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::shared::{ids::Id, time::TimestampMicros};

    fn mk_fn(source: &str, updated_micros: i64) -> Function {
        Function {
            id: Id("fn-1".into()),
            org_id: Id("orgA".into()),
            name: "redact_pw".into(),
            language: FunctionLanguage::Vrl,
            source: source.into(),
            params_schema: serde_json::Value::Null,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(updated_micros),
        }
    }

    #[tokio::test]
    async fn vrl_executor_runs_and_caches() {
        let ex = VrlFunctionExecutor::new();
        let f = mk_fn(r#"del(.pw)"#, 1);
        let mut ev = json!({ "user": "a", "pw": "x" });
        ex.run(&f, &mut ev).await.unwrap();
        assert!(ev.get("pw").is_none());
        // 再跑一次：应命中缓存
        let mut ev2 = json!({ "user": "b", "pw": "y" });
        ex.run(&f, &mut ev2).await.unwrap();
        assert!(ev2.get("pw").is_none());
    }

    #[tokio::test]
    async fn vrl_executor_invalidates_on_update() {
        let ex = VrlFunctionExecutor::new();
        // v1：del(.pw)
        ex.run(&mk_fn(r#"del(.pw)"#, 1), &mut json!({"pw": "a"}))
            .await
            .unwrap();
        // v2：del(.token)
        let mut ev = json!({"token": "t"});
        ex.run(&mk_fn(r#"del(.token)"#, 2), &mut ev).await.unwrap();
        assert!(ev.get("token").is_none());
    }

    #[tokio::test]
    async fn js_function_returns_invalid_until_feature_on() {
        let ex = VrlFunctionExecutor::new();
        let mut f = mk_fn(r#"function x(){}"#, 1);
        f.language = FunctionLanguage::Js;
        let err = ex.run(&f, &mut json!({})).await.unwrap_err();
        assert!(err.to_string().contains("javascript runtime"));
    }
}
