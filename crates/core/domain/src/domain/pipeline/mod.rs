// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Pipeline bounded context：写入时执行的实时流水线（[`realtime`]）、定时读取已存数据的流水线
//! （[`scheduled`]）、两者共用的步骤（[`processing`]），以及开事件前处理告警事件的流水线
//! （[`alert`]）。实时流水线不许改的字段见 [`protected`]。只在实时流水线里跑、跨事件计数的
//! 步骤：生成指标（[`metric`]）、配额与限流（[`limit`]），它们挑事件的条件见 [`condition`]。

pub mod alert;
pub mod condition;
pub mod limit;
pub mod metric;
pub mod processing;
pub mod protected;
pub mod realtime;
pub mod scheduled;
