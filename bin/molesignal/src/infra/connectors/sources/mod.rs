// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 由 MoleSignal 主动读取的日志来源：[`kafka`] 消费主题，[`s3`] 读新对象。它们读到的都写进连接器的
//! 目标日志流，写入流水线照常处理。

pub mod kafka;
pub mod s3;
