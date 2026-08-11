// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Notify 领域模型。
//!
//! 本上下文把企业发送凭证、用户接收端点、用户偏好和投递记录拆开。

pub mod connector;
pub mod delivery;
pub mod endpoint;
pub mod event;
pub mod policy;
pub mod preference;
pub mod recipient;
pub mod repositories;
pub mod routing;
pub mod template;
