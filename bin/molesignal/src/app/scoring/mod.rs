// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 的质量打分里不用 `AppState` 的部分：一个对象要排哪些打分（[`plan`]）、查询能不能
//! 解析（[`ParserSyntax`]）、分数写进 `_agent_score_traces` 的样子（[`score_event`]）。
//!
//! 执行打分、评审调模型与 Prompt 检查在 `api::http::routes::agent::quality`。

mod plan;
mod stream;
mod syntax;

pub use plan::{enqueue_answered, enqueue_rated, plan_answered, plan_rated};
pub use stream::score_event;
pub use syntax::ParserSyntax;
