# 当前后端结构

## 工作区形态

后端已经从约 20 个 workspace crate 合并为根 package `molesignal` 的单 crate：

```text
Cargo.toml
src/
  shared/
  domain/
  app/
  infra/
  api/
  bootstrap/
  protocol/
  config/
  intelligence/
  license/
  cloud_marketplace/
  domain_management/
  model_pricing/
  report_renderer/
  tantivy/
  sqlx-shim/
tests/
```

workspace member 只有：

- `.`：主 crate `molesignal`
- `src/sqlx-shim`：package 名为 `sqlx` 的受控 facade

不要使用旧的 `crates/domain`、`crates/app`、`crates/infra`、`crates/api`、`crates/bootstrap` 或 `crates/protoc` 路径。

## 逻辑分层

虽然模块位于同一 crate，仍按以下边界组织：

| 模块 | 主要职责 | 约束 |
|---|---|---|
| `src/shared` | `Error`、`Result`、`Id`、时间、health、metrics、License trait | 不依赖上层业务模块 |
| `src/domain` | 业务模型、领域 enum、repository/port trait | 不引入数据库、HTTP、gRPC、对象存储 adapter |
| `src/config` | Figment/TOML 配置 | 避免反向依赖运行时实现 |
| `src/app` | use case 与业务编排 | 优先通过 domain trait 注入外部能力 |
| `src/infra` | PostgreSQL、object store、DataFusion、Tantivy、WAL、通知与外部 adapter | 实现 domain port |
| `src/api` | axum/tonic 入口、认证授权、DTO 和协议转换 | 不把 transport 类型扩散到通用 domain API |
| `src/protocol` | buf 生成的 prost/tonic 类型 | 生成文件，不手工维护 |
| `src/bootstrap` | 进程角色、worker 生命周期、依赖装配 | `bootstrap.rs` 是总 composition root |

顶层的 `intelligence`、`license`、`cloud_marketplace`、`domain_management`、`model_pricing`、`report_renderer` 是已合并进主 crate 的产品模块，不再是私有或独立商业 crate。

## Repository 与装配

- repository trait 通常位于 `src/domain/<context>/`。
- PostgreSQL 实现位于 `src/infra/persistence/repositories/`。
- `MetaStore` 与 embedded migrations 位于 `src/infra/persistence/pool.rs`。
- service/use case 位于 `src/app/`。
- axum/tonic 入口位于 `src/api/http/` 与 `src/api/grpc/`。
- `src/bootstrap/bootstrap.rs` 只保留 `build_state` 总编排；repository、service 与 worker
  按功能在 `src/bootstrap/{core,storage,iam,intelligence,tracing,...}.rs` 实例化。
- 后台 loop 放在 `src/bootstrap/roles/` 或 `src/bootstrap/workers/`，由 bootstrap 管理生命周期。

## Protocol

- 源文件：`proto/**/*.proto`
- 生成配置：`proto/buf.gen.yaml`
- 生成目标：`src/protocol/`
- 命令：`make proto`
- `build.rs` 不生成 protobuf，只注入构建元数据。

修改 proto 后必须更新生成结果。新增生成模块时，还要检查 `src/protocol/mod.rs` 与相应 API 转换代码。

## Migration

- SQL 文件：`src/infra/migrations/*.sql`
- 注册点：`src/infra/persistence/pool.rs::embedded_migrator()`
- 运行方式：`include_str!` 编译期嵌入，再由 `Migrator::run` 执行

运行时不会扫描 migration 目录；新增 SQL 文件但漏掉注册会在部署后缺表。配套单测 `embedded_migrations_match_files_on_disk` 检查文件与注册列表一致。

## 测试

- unit test：源码内 `#[cfg(test)]`
- integration test：根 `tests/*.rs`
- 公共 fixture：`tests/common/mod.rs`
- Docker/testcontainers 测试：通常由 `MS_RUN_IT=1` 开启

旧的 `crates/<crate>/tests/it_*.rs` 和 `-p molesignal-bootstrap` 命令已经失效。

## 现有边界例外

`src/app/profiling.rs`、`profile_storage.rs`、`trace_export.rs`、`trace_candidate_router.rs` 与 self-telemetry 代码包含对 infra、protocol 或网络 adapter 的直接使用。这是当前工作树中的既有现实：

- 修改这些模块时保持改动局部，不把例外扩散为默认模式。
- 新增类似耦合时优先抽 port；确有必要则同步记录到 `ARCHITECTURE.md`。
- review 只报告当前 diff 新增的问题，不把未触及的基线问题归责于本次任务。

## License 形态

商业能力当前以运行时 `LicenseGate::has_feature` 为主，模块通常无条件编译。`CommunityLicense` 永远不开放商业 feature，`SignedLicense` 由 Ed25519 签名包激活，`LicenseHolder` 支持运行时替换。

不要要求已不存在的“商业 feature crate + 空 `cfg=`”模式。技术性 Cargo features（`ws`、`jemalloc`、`profiling-pprof`、`js-runtime`）与商业 License key 是两套机制。
