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
scripts/verify/verify.py <本 session 改过的文件...>
```

它按文件选 crate：`clippy` 查改动的 crate 及其下游，`cargo test --lib` 测改动的 crate 和读全仓源码的守卫 crate（`signals`、`postgres`），都用共享的 `target/verify`，重型步骤全机排队。

规则：

- 不在每个小改动后重复运行。
- 失败或中断后，修完重跑同一条命令，只补失败项和文件有变化的步骤。
- 不额外运行 `cargo build` 或 `cargo check`；`clippy` 和 `test` 已提供编译覆盖。
- 仅文档或非 Rust 配置修改不需要 Rust 编译验证。
- 全量（`make lint`、`make test`、`scripts/verify/verify.py --all`）只在用户明确要求时运行；日常全量由 CI 负责。

诊断单个测试：

```bash
scripts/verify/verify.py --run -- cargo test -p signals --lib every_spawned
```

## Integration Tests

主服务 integration test 位于 `bin/molesignal/tests/`。CI 使用：

```bash
MS_RUN_IT=1 cargo test -p molesignal --tests -- --test-threads=1
```

多数测试需要 Docker 与 testcontainers。只有变更涉及真实 PostgreSQL、完整 wire、HTTP/gRPC 服务或用户明确要求时，才把相关 integration test 纳入最终验证；不要在同一任务中反复询问或反复运行。

## Pre-commit

`.githooks/pre-commit` 只查暂存文件，依次执行：

1. license header check
2. nightly rustfmt `--check`
3. 前端类型检查（暂存了前端代码时）
4. 相关 crate 及其下游的 clippy（暂存了 Rust 相关文件时）

已经由 `scripts/verify/verify.py` 通过的内容不会重跑。

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
