// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Notify 连接器适配器与通用 SMTP sender。

pub mod adapters;
mod email;

pub use email::EmailSender;
