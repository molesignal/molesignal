// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! RUM 接收层。
//!
//! - [`normalize`]：RUM JSON → session/action/error 派生 stream 的 `RawEvent`。
//! - [`replay`]：`RumReplayWriter` 按组织、应用、session 和 seq 将 replay segment
//!   写入 object store，并把元数据写入 `rum_replay_events`。
//! - [`symbolication`]：解析 Web/Flutter/Android/iOS 调试产物并还原错误 frame。

pub mod normalize;
pub mod replay;
pub mod symbolication;
