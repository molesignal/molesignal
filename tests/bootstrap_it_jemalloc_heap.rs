// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! B3 heap profiling 端到端 IT（Linux + jemalloc）—— 不依赖 postgres / TestServer。
//!
//! 本测试二进制链 `molesignal` lib → 共享其 jemalloc 全局分配器与烘进的
//! `_rjem_malloc_conf`（含 `prof:true`）。两点都真实验证：
//!   1. 烘进的 malloc_conf 符号确被 jemalloc 读取 —— 否则 `opt.prof=false`，
//!      `dump_heap_profile()` 直接返 Err，下面的 `.expect()` 会炸。
//!   2. 采样开启后，`dump_heap_profile()` dump 出的 profile 真的包含其后做的存活分配
//!      的样本（第二次 dump 显著大于基线）。
//!
//! macOS / Windows / 非 jemalloc 构建：整条 cfg 排除，编出空 test 二进制（0 tests）。
//! 不需 testcontainers，故 CI 的 `cargo test -p molesignal` 会自动执行它，
//! 给"烘进符号是否仍被链接/读取"一份自动回归保护（弥补纯 example 只编不跑的盲区）。
//!
//! 本机贴生产 glibc 手动跑：
//!   docker run --rm -v "$PWD":/src -w /src \
//!     -v molesignal_alloc_cargo:/usr/local/cargo/registry \
//!     -v molesignal_alloc_target:/target -e CARGO_TARGET_DIR=/target \
//!     -e CARGO_PROFILE_RELEASE_LTO=false -e CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
//!     rust:1.96-bookworm \
//!     cargo test --release -p molesignal --test bootstrap_it_jemalloc_heap -- --nocapture

#[cfg(all(target_os = "linux", target_env = "gnu", feature = "jemalloc"))]
#[test]
fn heap_profile_captures_live_allocations() {
    // 强制把 molesignal lib 链入本测试二进制——本测试只用 molesignal::api，
    // 不调它就不会链入 lib 的 #[global_allocator] 与烘进符号，opt.prof 会是 false。
    molesignal::bootstrap::allocator::ensure_malloc_conf_linked();

    // 1. 先 dump 一次：dump_heap_profile() 内部把 prof.active 翻成 true（打开采样），
    //    同时拿到“基线” profile——此刻我们的存活分配还没做，故基线里几乎没有我们的样本。
    //    若 _rjem_malloc_conf 没被读取，opt.prof=false → 这里返 Err → expect 炸（即想要的失败信号）。
    let baseline = molesignal::api::dump_heap_profile().expect(
        "baseline dump failed (opt.prof=false ⇒ baked _rjem_malloc_conf not read by jemalloc)",
    );
    assert!(!baseline.is_empty(), "baseline profile is empty");
    let head = String::from_utf8_lossy(&baseline[..baseline.len().min(16)]).to_string();
    assert!(
        head.contains("heap"),
        "expected jemalloc heap profile header, got prefix: {head:?}"
    );

    // 2. 采样已开 → 现在做一批存活分配（~64MB），这些会被 jemalloc 采样记录进堆 profile。
    let mut live: Vec<Vec<u8>> = Vec::with_capacity(8000);
    for i in 0..8000u32 {
        live.push(vec![(i & 0xff) as u8; 8192]);
    }

    // 3. 再 dump：应含上面这批分配的样本 → 显著大于基线。
    let after = molesignal::api::dump_heap_profile().expect("second dump failed");
    std::hint::black_box(&live); // 防存活分配在 dump 前被优化释放

    assert!(
        after.len() > baseline.len(),
        "active-window allocations should add heap samples: after={} bytes, baseline={} bytes",
        after.len(),
        baseline.len()
    );
}
