# Validation Record

Date: 2026-07-27

## Passed

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Passed. The stable toolchain reports that two nightly-only rustfmt options are ignored. |
| `git diff --check -- <change files>` | Passed. |
| `cargo check --all-targets` | Passed. |
| `cargo check --no-default-features --lib` | Passed. |
| `cargo test --offline --lib self_telemetry` | Passed: 16 tests. |
| `cargo test --offline --lib profiling` | Passed: 9 tests, including a decodable native CPU pprof capture. |
| `cargo test --offline --test bootstrap_it_self_telemetry` | Passed and compiled the opt-in standalone integration test target. |
| `cargo test --offline` | Passed: 848 library tests plus every enabled integration and documentation test. |
| `cargo clippy --offline --all-targets -- -D warnings -A clippy::too-many-arguments` | Passed. The sole temporary allowance is for an unrelated, pre-existing dirty-worktree change in `src/infra/notify/mod.rs`. |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | Passed without lint allowances after downloading the explicitly authorized optional dependency graph. |
| `cargo test --all-features --locked infra::runtime::js_executor::tests` | Passed: 4 JS runtime tests; unrelated test targets were filtered out. |

## All-Features Follow-Up

With explicit authorization on 2026-07-27, Cargo downloaded the missing optional dependency
graph, including `az 1.3.0`, and ran the required build scripts and procedural macros. The exact
all-features Clippy gate now passes without any lint allowance. Its first run exposed two
`js-runtime`-only `collapsible_if` diagnostics; the equivalent cleanup and four focused runtime
tests both passed.

The same follow-up removed the development-stage configurable system-organization slug.
Runtime targeting is fixed to `SYSTEM_ORG_SLUG = "_sys"`; the removed field is rejected as
unknown. Config, default-config, `_sys` stream-bootstrap, formatting, Clippy, and strict OpenSpec
checks all pass.

## Platform-Gated Coverage

- The Docker-backed branch of `bootstrap__it_self_telemetry` requires `MS_RUN_IT=1`; Docker is unavailable on this host, so the target compiled and its normal gated path passed, but the real-container branch did not execute.
- Native jemalloc heap capture is supported only on Linux glibc builds. This macOS run verified the explicit unsupported response and the fixture-based canonical pprof conversion; the native Linux heap integration test was not compiled for this target.
- Release target frame-pointer settings are encoded in `.github/workflows/release.yml`. Cross-target native release builds remain delegated to that CI matrix.
