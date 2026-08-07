// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Server bootstrap lib 入口；`molesignal` 二进制和集成测试共用这里的
//! [`build_state`] 与 role 启动函数。

mod alerting;
#[path = "bootstrap.rs"]
mod composition;
mod core;
mod iam;
mod intelligence;
mod license;
pub mod llm_executor;
mod platform;
mod query;
pub mod roles;
mod storage;
mod tracing;
pub mod workers;

pub mod acme;
pub mod tls;

// 全局分配器：生产 Linux 默认 jemalloc，dev/macOS 退回系统默认（见 allocator.rs）。
// 放 lib 而非二进制入口：`molesignal` 二进制、集成测试二进制都链本 lib → 共享同一个 jemalloc
// 全局分配器（每个最终 artifact 仍只此一处定义 `#[global_allocator]`），让 heap-profiling
// 端点能在 Linux IT 里被真实验证。
//
// 注意：lib 里的 `#[global_allocator]` 与烘进的 `_rjem_malloc_conf` 符号只有在下游目标
// 确实链入本 lib 时才生效——`molesignal` 二进制天然满足；不引用本 lib 的集成测试须显式调
// `allocator::ensure_malloc_conf_linked()` 强制链入。
pub mod allocator;

pub use composition::{activate_self_telemetry, build_state, rewrap_kek};
