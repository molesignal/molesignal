# 当前后端结构

## 工作区形态

仓库根 `Cargo.toml` 是 virtual workspace，不再承载 package：

```text
Cargo.toml
bin/
  molesignal/                 # 最终服务 package；api/app/bootstrap 与剩余 adapters
  probe-agent/                # 最终 Probe Agent package
crates/
  core/
    contracts/ kernel/ domain/ protocol/ settings/
  engines/
    function-runtime/ index-format/ postgres/ profiles/
    query-language/ service-graph/
  modules/
    agent/ cloud-marketplace/ domain-management/ license/ model-pricing/
  transport/
    grpc-client/
  support/
    permission-macro/ report-renderer/ signals/ sqlx-shim/
tools/
  license-sign/
```

命名规则：分类目录使用 `modules`；PostgreSQL engine 名为 `postgres`；可观测性 support
crate 名为 `signals`；`sqlx-shim` 保持原名。除最终产品名外，单一职责 crate 不使用
`molesignal-` 前缀。

## 依赖方向

- `bin/molesignal` 是 composition root，可依赖 workspace 中的 core、engine、module、transport 与 support crate。
- `crates/core/domain` 持有业务模型和 port/repository trait，不依赖数据库、HTTP、gRPC 或对象存储 adapter。
- `crates/engines/postgres` 持有 PostgreSQL repository、migration、持久化加密和 Probe CA 存储。
- `crates/modules/*` 持有可独立演进的产品能力，优先依赖 core port，而不是应用 package。
- `crates/transport/*` 封装协议客户端边界；`crates/support/*` 承载跨域技术设施。
- `bin/molesignal/src/api` 负责 axum/tonic 入口和协议转换；`src/app` 负责用例编排；
  `src/bootstrap` 管理依赖装配、进程角色与 worker 生命周期。
- 尚未独立的 object store、DataFusion、Tantivy、WAL、通知等 adapter 暂留
  `bin/molesignal/src/infra`，后续只有形成稳定职责边界时再提取。

`bin/molesignal/src/{domain,config,protocol,shared}` 以及部分 `infra` 模块是兼容 facade，
用于保持现有应用代码的 import 稳定；新 crate 间依赖应直接引用实际 crate，避免通过
`molesignal` 反向依赖 composition root。

## Repository 与装配

- repository trait 通常位于 `crates/core/domain/src/domain/<context>/`。
- PostgreSQL 实现位于 `crates/engines/postgres/src/persistence/repositories/`。
- `MetaStore` 与 embedded migrations 位于
  `crates/engines/postgres/src/persistence/pool.rs`。
- service/use case 位于 `bin/molesignal/src/app/`；真正可独立复用的能力可提升到对应 module 或 engine crate。
- axum/tonic 入口位于 `bin/molesignal/src/api/http/` 与 `bin/molesignal/src/api/grpc/`。
- `bin/molesignal/src/bootstrap/bootstrap.rs` 保留 `build_state` 总编排；repository、service
  与 worker 按功能在同目录分段装配。
- 后台 loop 放在 `bin/molesignal/src/bootstrap/roles/` 或 `workers/`，由 bootstrap 管理生命周期。

## Protocol

- 源文件：`proto/**/*.proto`
- schema 校验：`make proto-lint`（Buf）
- 服务端 codegen：`crates/core/protocol/build.rs` → Cargo `OUT_DIR`
- Probe codegen：`bin/probe-agent/build.rs` → Cargo `OUT_DIR`
- `bin/molesignal/build.rs` 只注入构建元数据，不生成 protobuf。

生成 `.rs` 是 `target/` 下的构建产物，不进入源码树，也不提交 Git。修改 proto 后必须
通过 schema 校验与 Rust 编译；新增 package 时还要更新 `crates/core/protocol/src/lib.rs`、
对应 crate 的 `build.rs`、Probe module 声明和相应 API 转换代码。

## Migration

- SQL 文件：`crates/engines/postgres/src/migrations/*.sql`
- 注册点：`crates/engines/postgres/src/persistence/pool.rs::embedded_migrator()`
- 运行方式：`include_str!` 编译期嵌入，再由 `Migrator::run` 执行

首次发布前只保留三个有明确职责的基线文件：`initial` 承载领域 schema，
`builtin_dashboards` 承载系统 Dashboard，`iam_route_catalog` 承载 Route 与导航访问目录。
开发期新增领域表、约束和权限继续折叠进 `initial`；Route seed 进入 Route Catalog。

运行时不会扫描 migration 目录；新增 SQL 文件但漏掉注册会在部署后缺表。配套单测
`embedded_migrations_match_files_on_disk` 检查文件与注册列表一致。

## 测试

- unit test：各 crate 源码内 `#[cfg(test)]`
- 主服务 integration test：`bin/molesignal/tests/*.rs`
- 公共 fixture：`bin/molesignal/tests/common/mod.rs`
- Docker/testcontainers 测试：通常由 `MS_RUN_IT=1` 开启

主服务测试使用 `cargo test -p molesignal`；workspace 全量单元测试使用 `make test`。

## 现有边界例外

`bin/molesignal/src/app` 中 profiling、trace export、candidate router 与 self-telemetry
仍直接使用部分 adapter 或 transport。这是渐进迁移中的既有现实：

- 修改这些模块时保持改动局部，不把例外扩散为默认模式。
- 新增类似耦合时优先抽 domain port；确有必要则同步记录到 `ARCHITECTURE.md`。
- review 只报告当前 diff 新增的问题，不把未触及的基线问题归责于本次任务。

## License 形态

商业能力以运行时 `LicenseGate::has_feature` 为主，模块通常无条件编译。
`CommunityLicense` 永远不开放商业 feature，`SignedLicense` 由 Ed25519 签名包激活，
`LicenseHolder` 支持运行时替换。

技术性 Cargo features（`ws`、`jemalloc`、`profiling-pprof`、`js-runtime`）与商业
License key 是两套机制。
