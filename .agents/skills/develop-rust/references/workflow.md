# Rust 后端工作流

## 工具链

- stable Rust：`1.96`，由 `rust-toolchain.toml` 锁定；编译、clippy 和 test 使用它。
- nightly：仅用于 rustfmt，因为 `rustfmt.toml` 使用 nightly-only 选项。
- 安装 nightly rustfmt：

```bash
rustup toolchain install nightly --profile minimal --component rustfmt
```

## 单轮收尾验证

先完成任务范围内全部修改，再执行一轮：

```bash
make fmt
make check-license-headers
make lint
make test
```

规则：

- 不在每个小改动后重复运行上述命令。
- 不额外运行 `cargo build` 或 `cargo check`；`clippy` 和 `test` 已提供编译覆盖。
- 仅文档或非 Rust 配置修改不需要 Rust 编译验证。
- 若最终验证失败，先集中修复所有已发现问题，再只重跑失败项。
- 如果任务明确要求更窄或更广的验证，以用户要求为准，但仍只做一次完整收尾。

`make test` 对应：

```bash
cargo test --frozen --locked --workspace --lib --bins
```

`make lint` 对应：

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

## Integration Tests

主服务 integration test 位于 `bin/molesignal/tests/`。CI 使用：

```bash
MS_RUN_IT=1 cargo test -p molesignal --tests -- --test-threads=1
```

多数测试需要 Docker 与 testcontainers。只有变更涉及真实 PostgreSQL、完整 wire、HTTP/gRPC 服务或用户明确要求时，才把相关 integration test 纳入最终验证；不要在同一任务中反复询问或反复运行。

## Pre-commit

`.githooks/pre-commit` 依次执行：

1. license header check
2. nightly `cargo fmt --check`
3. workspace clippy

安装：

```bash
make install-hooks
```

## Proto

修改 `proto/**/*.proto` 后执行一次：

```bash
make proto-lint
```

该命令通过 Buf 校验 schema。`crates/core/protocol/build.rs` 与
`bin/probe-agent/build.rs` 会在 Cargo 构建时调用 `tonic-prost-build`，将 Rust binding
写入各自的 `OUT_DIR`。

检查：

- `crates/core/protocol/src/lib.rs` 与 Probe module 是否仍导出正确模块。
- API/gRPC adapter 是否适配字段或 service 变化。
- 生成 `.rs` 不得进入源码树或 Git。

## Database Migration

项目首次发布前只维护三个职责明确的基线 migration：

```text
crates/engines/postgres/src/migrations/20260101000001_initial.sql
crates/engines/postgres/src/migrations/20260101000002_builtin_dashboards.sql
crates/engines/postgres/src/migrations/20260101000003_iam_route_catalog.sql
```

开发期 schema 变更继续折叠进 `20260101000001_initial.sql`。内置 Dashboard
记录和完整指标目录只修改 `20260101000002_builtin_dashboards.sql`；Route、导航分组与
Route Permission seed 只修改 `20260101000003_iam_route_catalog.sql`。不要在首次发布前
继续拆出增量 migration。`crates/engines/postgres/src/persistence/pool.rs::embedded_migrator()`
必须显式注册这三个文件；运行时不会自动扫描目录。

首次正式发布后不得再修改已发布 migration，届时改为新增
`YYYYMMDDHHMMSS_<name>.sql` 并同步注册。需要非事务执行时，文件必须以
`-- no-transaction` 开头，并确认 DDL 支持失败恢复。

`embedded_migrations_match_files_on_disk` 会校验磁盘文件与注册列表完全一致。

## CI 对应关系

- `.github/workflows/rust.yml`：proto、fmt-check、clippy、unit/bin tests
- `.github/workflows/it.yml`：编译并运行 `bin/molesignal/tests/` integration tests
- `Makefile`：本地统一入口

更新 Rust 版本、proto 生成、测试命令或目录结构时，同步这些文件和本参考。
