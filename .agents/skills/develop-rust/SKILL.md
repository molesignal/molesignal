---
name: develop-rust
description: Implement, modify, debug, or refactor the MoleSignal Rust backend using its current virtual-workspace architecture, project domain types, generated protocol flow, migrations, and one-pass validation. Use for changes under bin/**/*.rs, crates/**/*.rs, tools/**/*.rs, Cargo.toml, proto/**/*.proto, crates/engines/postgres/src/migrations/*.sql, backend Makefile targets, or Rust backend features and bug fixes.
---

# 开发 Rust 后端

按当前仓库结构完成后端修改，并把分析、实现与最终验证集中在一次工作流中。

## 执行流程

1. 先读取用户指定范围、当前工作树和相关源码；保留所有不属于当前任务的未提交修改。
2. 根据任务读取下方参考文件，不要无条件加载全部资料。
3. 一次性完成当前任务范围内的实现、测试、生成文件和文档更新。
4. 处理 proto 或 migration 等必须同步的产物与注册点。
5. 全部修改结束后，仅执行一轮收尾验证；遵守根目录 `AGENTS.md` 的验证频率规则。

## 按需读取参考

- 修改模块边界、repository、service、HTTP/gRPC、worker、bootstrap、protocol 或顶层产品模块时，读取 [project-architecture.md](references/project-architecture.md)。
- 编写或修改 Rust 代码、测试、错误处理、日志、时间字段、并发或序列化时，读取 [rust-standards.md](references/rust-standards.md)。
- 涉及验证命令、工具链、proto、migration、integration test、CI 或 pre-commit 时，读取 [workflow.md](references/workflow.md)。
- 跨层后端功能通常需要读取三份参考；窄范围修改只读取真正相关的文件。

## 实现约束

- 以后端当前的 virtual workspace 为准：最终 package 位于 `bin/`，可复用 crate 位于 `crates/`，开发工具位于 `tools/`。
- 新的单一职责 crate 不使用 `molesignal-` 前缀；按 `core`、`engines`、`modules`、`transport`、`support` 分类放置。
- 优先复用已有领域类型、repository trait、service 和错误类型，不创建平行抽象。
- 除生成代码和测试代码外，新建生产实现文件不得超过 500 行；生产源码内的测试模块不计入上限。
- 当前任务实质修改已有超限文件时，按职责拆出本次涉及的内聚逻辑；极小孤立修复不要求重构无关历史代码，但不得继续堆叠独立职责。
- 同一功能或模块拆成两个及以上实现文件时，建立专属目录集中放置，禁止将同前缀文件散落在父目录，也禁止使用无明确归属的 `utils`、`helpers`、`common` 或 `misc` 作为容器。
- 新增 repository 实现或运行时依赖时，检查 `bin/molesignal/src/bootstrap/bootstrap.rs` 的总编排及对应功能装配文件。
- 修改 `proto/**/*.proto` 后运行一次 `make proto-lint`。服务端与 Probe binding 由各自
  `build.rs` 自动生成到 Cargo `OUT_DIR`，生成 `.rs` 不进入源码树，也不提交 Git。
- 首次发布前只维护 `initial`、`builtin_dashboards` 与 `iam_route_catalog` 三个基线 migration；
  领域 schema 折叠进 `initial`，系统 Dashboard 与 Route Catalog 各自留在专属文件。
  首次发布后新增 migration 时，再同步注册到
  `crates/engines/postgres/src/persistence/pool.rs::embedded_migrator()`。
- 不因发现一个小问题就提前运行整套 `test`、`clippy`、`build` 或 `check`。

## 收尾

- 先集中解决静态检查能预见的问题，再进入最终验证。
- `cargo test` 或 `cargo clippy` 已完成编译覆盖时，不额外运行 `cargo build` 或 `cargo check`。
- 验证通过后停止重复检查；若失败并完成修复，只重跑失败项。
