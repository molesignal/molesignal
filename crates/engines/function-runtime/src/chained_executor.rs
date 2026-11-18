// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Combines three executors (VRL, optional JS, optional LLM) into a single `FunctionExecutor`.
//!
//! The Wire layer always injects this struct:
//! - `feature = "js-runtime"` off -> `js = None`, and every row with `language = Js` is
//!   rejected (`IntakeError { reason: "javascript runtime disabled" }`).
//! - `feature = "js-runtime"` on (the default for the main binary) ->
//!   `js = Some(JsFunctionExecutor)`, which dispatches to a V8 isolate.
//! - `[functions].llm_eval_enabled = true` -> `llm = Some(...)` (injected by bootstrap,
//!   including the agent provider), and rows with `language = Llm` are evaluated by calling
//!   the model; otherwise they are rejected.
//!
//! This layer deliberately caches nothing; the compile cache lives in the underlying executors.

use std::sync::Arc;

use async_trait::async_trait;
use domain::function::{Function, FunctionExecutor, FunctionLanguage, FunctionOutcome};
use kernel::{Error, Result};

use crate::vrl::executor::VrlFunctionExecutor;

pub struct ChainedFunctionExecutor {
    vrl: Arc<VrlFunctionExecutor>,
    /// A trait object lets the OSS and paid editions inject different backends;
    /// always None when the feature is off.
    js: Option<Arc<dyn FunctionExecutor>>,
    /// LLM evaluation executor (injected by bootstrap; requires an agent provider);
    /// None when disabled.
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
    async fn run(
        &self,
        function: &Function,
        event: &mut serde_json::Value,
    ) -> Result<FunctionOutcome> {
        self.run_with_metadata(function, event, &serde_json::Map::new())
            .await
    }

    async fn run_with_metadata(
        &self,
        function: &Function,
        event: &mut serde_json::Value,
        metadata: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<FunctionOutcome> {
        match function.language {
            FunctionLanguage::Vrl => self.vrl.run_with_metadata(function, event, metadata).await,
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
    use kernel::{ids::Id, time::TimestampMicros};
    use serde_json::json;

    use super::*;

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
