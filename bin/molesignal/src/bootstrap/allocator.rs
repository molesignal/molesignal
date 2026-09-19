// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 全局分配器：生产 glibc Linux 默认 jemalloc，musl / dev / macOS 退回系统默认。
//!
//! 背景：本服务在长驻容器里跑 Arrow / DataFusion / tantivy 这类高 churn 分配负载，
//! glibc ptmalloc2 的通病是「峰值过后内存不还给 OS」（RSS plateau），在硬内存
//! limit 的容器里既抬高部署成本又增加 OOM 风险。`scripts/alloc_bench.sh` 在真实
//! glibc Linux（arm64 容器，allocation-stress 负载）实测三档对照：
//!
//! | 配置                          | 吞吐 q/s | 峰值 RSS | idle 后 RSS |
//! |-------------------------------|----------|----------|-------------|
//! | glibc 默认                    | 469      | 372 MB   | 304 MB      |
//! | glibc ARENA_MAX=2 + trim      | 85 ⚠     | 235 MB   | 99 MB       |
//! | jemalloc (background_thread)  | 569      | 544 MB   | 56 MB       |
//!
//! jemalloc 相对 glibc 默认：吞吐 +21%，idle 后 RSS −82%（304→56MB）。glibc 单独
//! 调 `ARENA_MAX=2` 虽降 RSS 但吞吐暴跌（arena 锁竞争），不可取。结论：换 jemalloc。
//!
//! 仅在 **glibc Linux**（`target_env = "gnu"`，生产目标）生效；musl / macOS / Windows
//! 退回系统分配器、不编 jemalloc C 源（cfg 门控）。收窄到 gnu 的原因：tikv-jemalloc-sys
//! 在 musl 上编不过（jemalloc 的 atomics 检测失败）且 `background_thread` 不受 musl 支持，
//! 所以对外只在 glibc 上发 jemalloc —— release tarball 用 `*-unknown-linux-gnu`，Docker
//! 用 debian bookworm（即 glibc）。`jemalloc` feature 默认开，`--no-default-features`
//! 可在 glibc 上构出不带 jemalloc 的二进制做 A/B 对照。

/// 生产默认 `_rjem_malloc_conf`。
///
/// tikv-jemallocator 默认带 `_rjem_` 前缀构建，故配置符号与环境变量都带前缀
/// （`_RJEM_MALLOC_CONF`）—— 这与 `scripts/alloc_bench.sh` 验证档 ③ 用的
/// `_RJEM_MALLOC_CONF=...` 完全一致，烘进二进制即把那组验证过的配置变成默认。
///
/// - `background_thread:true` —— 后台线程主动 purge 空闲页还给 OS（idle RSS
///   304→56MB 的关键）。
/// - `dirty_decay_ms:1000,muzzy_decay_ms:1000` —— decay 调到 1s（bench 验证档用的
///   就是这组），控住峰值 RSS；容器内存 limit 紧时可经环境变量再调小。
/// - `prof:true,prof_active:false` —— 把 heap profiling 机能编入但默认不采样
///   （稳态零开销），供 `/api/v1/debug/profile/heap`（B3）。注意：**首次**命中该端点会
///   打开采样（`prof.active=true`）并**保持到进程结束**，之后每次返回累积 profile；即
///   “零开销直到首次请求 profiling”，而非每次请求后自动复位（要复位需重启进程）。
///
/// 运行期 `_RJEM_MALLOC_CONF` 环境变量优先级高于本符号，可整体覆盖。
///
/// 以 `&[u8]`（NUL 结尾）导出：其内存布局首字段即数据指针，jemalloc 按
/// `const char *` 只读首字 → 命中字符串；`&[u8]` 满足 `Sync`、布局稳定，是社区
/// 验证过的惯用法。`#[used]` 防止未被 Rust 引用时被链接器剔除。
#[cfg(all(target_os = "linux", target_env = "gnu", feature = "jemalloc"))]
#[used]
#[unsafe(export_name = "_rjem_malloc_conf")]
pub static MALLOC_CONF: &[u8] =
    b"background_thread:true,dirty_decay_ms:1000,muzzy_decay_ms:1000,prof:true,prof_active:false\0";

#[cfg(all(target_os = "linux", target_env = "gnu", feature = "jemalloc"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

/// 显式触达烘进的 `_rjem_malloc_conf` 符号，确保它无论 `#[used]` / 链接器 GC / `lto`
/// 行为如何都被保留并参与最终链接。
///
/// 该符号挂在本 lib，靠下游目标链入本 lib 才生效；这一引用让"保留"不依赖 `#[used]`
/// 的实现细节（生产 `[profile.release]` 使用 `lto = "thin"` 并保留 profiling 符号）。
/// 非 Linux / 非 jemalloc 构建为 no-op。
///
/// 用法：
/// - bin 的 `main` 早期调一次（纯链接保留用途，无运行期副作用——jemalloc 在 main 前就已读取本符号）；
/// - 不引用本 lib 的集成测试调它，把本 lib（连同 jemalloc 全局分配器与本符号）强制链入。
#[inline(never)]
pub fn ensure_malloc_conf_linked() {
    #[cfg(all(target_os = "linux", target_env = "gnu", feature = "jemalloc"))]
    std::hint::black_box(MALLOC_CONF.as_ptr());
}
