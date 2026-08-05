// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 把 VRL + 可选 JS + 可选 LLM 三路 executor 组合成单一 `FunctionExecutor`。
//!
//! Wire 层始终注入此结构：
//! - `feature = "js-runtime"` 关 → `js = None`，所有 `language = Js` 的 row
//!   被拒（`IngestError { reason: "javascript runtime disabled" }`）。
//! - `feature = "js-runtime"` 开 + `[functions].js_runtime_enabled = true` →
//!   `js = Some(JsFunctionExecutor)`，分发到 V8 isolate。
//! - `[functions].llm_eval_enabled = true` → `llm = Some(...)`（bootstrap 注入，含
//!   intelligence provider），`language = Llm` 的 row 调模型评估；否则被拒。
//!
//! 这层故意不缓存任何东西；compile cache 在底层 executor 里。

use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    app::ingestion::FunctionExecutor,
    domain::function::{Function, FunctionLanguage},
    infra::runtime::vrl::executor::VrlFunctionExecutor,
    shared::{Error, Result},
};

pub struct ChainedFunctionExecutor {
    vrl: Arc<VrlFunctionExecutor>,
    /// 用 trait object 让 OSS / 付费版可以注入不同后端；feature off 时永远 None。
    js: Option<Arc<dyn FunctionExecutor>>,
    /// LLM 评估执行器（bootstrap 注入，需 intelligence provider）；关闭时 None。
    llm: Option<Arc<dyn FunctionExecutor>>,
}

impl ChainedFunctionExecutor {
    pub fn new(
        vrl: Arc<VrlFunctionExecutor>,
        js: Option<Arc<dyn FunctionExecutor>>,
        llm: Option<Arc<dyn FunctionExecutor>>,
    ) -> Self {
        Self { vrl, js, llm }
    }

    pub fn vrl_only(vrl: Arc<VrlFunctionExecutor>) -> Self {
        Self {
            vrl,
            js: None,
            llm: None,
        }
    }
}

#[async_trait]
impl FunctionExecutor for ChainedFunctionExecutor {
    async fn run(&self, function: &Function, event: &mut serde_json::Value) -> Result<()> {
        match function.language {
            FunctionLanguage::Vrl => self.vrl.run(function, event).await,
            FunctionLanguage::Js => match &self.js {
                Some(js) => js.run(function, event).await,
                None => Err(Error::invalid("javascript runtime disabled")),
            },
            FunctionLanguage::Llm => match &self.llm {
                Some(llm) => llm.run(function, event).await,
                None => Err(Error::invalid("llm eval runtime disabled")),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::shared::{ids::Id, time::TimestampMicros};

    fn js_fn() -> Function {
        Function {
            id: Id("fn".into()),
            org_id: Id("org".into()),
            name: "js".into(),
            language: FunctionLanguage::Js,
            source: "noop".into(),
            params_schema: serde_json::Value::Null,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(1),
        }
    }

    #[tokio::test]
    async fn js_without_executor_returns_disabled() {
        let chained = ChainedFunctionExecutor::vrl_only(Arc::new(VrlFunctionExecutor::new()));
        let mut ev = json!({});
        let err = chained.run(&js_fn(), &mut ev).await.unwrap_err();
        assert!(
            err.to_string().contains("javascript runtime disabled"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn vrl_path_still_works() {
        let chained = ChainedFunctionExecutor::vrl_only(Arc::new(VrlFunctionExecutor::new()));
        let f = Function {
            id: Id("fn".into()),
            org_id: Id("org".into()),
            name: "vrl".into(),
            language: FunctionLanguage::Vrl,
            source: "del(.pw)".into(),
            params_schema: serde_json::Value::Null,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(1),
        };
        let mut ev = json!({"pw": "x", "user": "a"});
        chained.run(&f, &mut ev).await.unwrap();
        assert!(ev.get("pw").is_none());
        assert_eq!(ev["user"], "a");
    }
}
