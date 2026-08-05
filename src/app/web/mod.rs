// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Web shell use-case 层（spec web-investigation-shell）。
//!
//! 当前状态：handler 已经是「extract → call repo → return DTO」薄一层，业务逻辑
//! （topology 聚合、span 树构造、correlation provider 派生）仍内联于 `api` crate
//! 的 routes/web/* 模块里。把它们提到这里需要：
//!
//! 1. 引入 `WebSearchHit` / `TopologyResponse` / `Span` 等 DTO 到 app 层；
//! 2. handler 收薄到 `Json(state.app.web.topology(...).await?)`；
//! 3. 单测从 axum IO 解耦，纯结构变换可单独覆盖。
//!
//! 不阻塞前端联调；移植窗口安排在 follow-up（与 web shell M2 一起）。
//!
//! 本文件留作上述移植的目标位置，确保 app crate 内已有该 mod 路径供调用方先 import。

pub mod aggregation;
pub mod correlation;
pub mod investigation_blob;
pub mod search;
pub mod topology;
pub mod trace;
