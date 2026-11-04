// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 流水线的 Postgres 存储。流水线本身在 `scheduled_pipelines`（见 `infra::pipeline`），这里是
//! 定时流水线的运行记录。

pub mod runs;
