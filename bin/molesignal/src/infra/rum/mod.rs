// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! RUM 接收层。
//!
//! - [`analytics`]：漏斗、路径与留存——对会话、视图和操作提的产品分析问题，各是一条 SQL。
//! - [`alerts`]：告警规则盯的 RUM 指标——按页面读数的同一套聚合取数，并找出事件要带的样本会话。
//! - [`intake`]：`POST /api/v1/rum/events` 的事件入口——解码、按契约校验、展平成各流的行、
//!   用连接信息（地理位置、浏览器）补全，并把 view 作为补丁送去派生会话。
//! - [`query`]：探索页与详情页用的 SQL 构造（过滤、分页、facet、直方图、实体去重）。
//! - [`issues`]：错误按 issue 归并——错误所属 issue 的 id、写入端的追踪器（新 issue、再现、
//!   回归、每小时新增上限）和应用的版本先后。
//! - [`session_state`]：会话合并器，把 view 补丁合并成 `rum_sessions` 的行。
//! - [`replay`]：`RumReplayWriter` 按组织、应用、session 和 seq 将 replay segment
//!   写入 object store，并把元数据写入 `rum_replay_events`。
//! - [`symbolication`]：解析 Web/Flutter/Android/iOS 调试产物并还原错误 frame。

pub mod alerts;
pub mod analytics;
pub mod intake;
pub mod issues;
pub mod query;
pub mod replay;
pub mod session_state;
pub mod symbolication;
