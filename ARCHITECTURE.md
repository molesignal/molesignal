# MoleSignal Rust Workspace 架构

MoleSignal 后端使用 virtual Cargo workspace。仓库根 `Cargo.toml` 只管理 workspace，
最终可执行 package 统一放在 `bin/`，可复用 crate 放在 `crates/`，开发工具放在
`tools/`。

## 完整形态

```text
.
├── Cargo.toml                         # virtual workspace
├── bin/
│   ├── molesignal/                    # 主服务 package + binary
│   │   ├── build.rs
│   │   ├── src/
│   │   │   ├── api/                   # HTTP/gRPC transport entrypoints
│   │   │   ├── app/                   # application services/use cases
│   │   │   ├── bootstrap/             # composition root, roles, workers
│   │   │   └── infra/                 # 尚未独立的 adapters
│   │   ├── tests/
│   │   ├── benches/
│   │   └── examples/
│   └── probe-agent/                   # Probe Agent package + binary
├── crates/
│   ├── core/
│   │   ├── contracts/                 # 稳定 schema 与 validator
│   │   ├── kernel/                    # Error/Result/Id/time/health/license primitives
│   │   ├── domain/                    # 领域模型与 ports/repository traits
│   │   ├── protocol/                  # build.rs 生成到 OUT_DIR 的 protobuf/gRPC 类型
│   │   └── settings/                  # 类型化配置与加载
│   ├── engines/
│   │   ├── function-runtime/          # VRL/可选 JS function runtime
│   │   ├── index-format/              # Puffin/Tantivy index format
│   │   ├── postgres/                  # PostgreSQL repositories/migrations/cipher/Probe CA
│   │   ├── profiles/                  # profiling model 与 codec
│   │   ├── query-language/            # SQL AST/query language helpers
│   │   └── service-graph/             # service graph aggregation 与 persistence
│   ├── modules/
│   │   ├── agent/
│   │   ├── cloud-marketplace/
│   │   ├── domain-management/
│   │   ├── license/
│   │   └── model-pricing/
│   ├── transport/
│   │   └── grpc-client/               # 共享 gRPC client/channel primitives
│   └── support/
│       ├── permission-macro/
│       ├── report-renderer/
│       ├── signals/                   # logs/metrics/tracing/self-telemetry
│       └── sqlx-shim/                 # Postgres-only traced sqlx facade
└── tools/
    └── license-sign/                  # license 签发 CLI
```

## 命名规则

- 业务能力分类使用 `modules`，不使用 `capabilities` 作为 crate 分类名。
- PostgreSQL engine 使用 `postgres`，不使用 `persistence-pg`。
- 可观测性 support crate 使用 `signals`，不使用 `observability`。
- `sqlx-shim` 保持原名。
- 除最终产品 package（如 `molesignal`）外，单一职责 crate 不带 `molesignal-` 前缀。
- 最终可执行 package 只放在 `bin/`；不可执行库不放入 `bin/`。

## 依赖方向

`bin/molesignal` 是总 composition root，可以依赖 workspace 内各类 crate。其余 crate
不能反向依赖 `molesignal`：

```text
bin/molesignal ──► modules / engines / transport / support / core
modules        ──► core / support
engines        ──► core / support
transport      ──► protocol/runtime dependencies
```

`crates/core/domain` 只持有业务模型与 port，不引入 PostgreSQL、HTTP、gRPC 或对象存储
adapter。PostgreSQL 实现位于 `crates/engines/postgres`。应用 use case、外部 API 和进程
生命周期分别由 `bin/molesignal/src/app`、`api`、`bootstrap` 管理。

主 package 中的 `domain`、`config`、`protocol`、`shared` 以及部分 `infra` module 是
迁移期兼容 facade，用于让现有应用 import 保持稳定。workspace crate 之间应直接依赖
实际 crate，不应经由这些 facade 形成对主 package 的反向依赖。

## 生成与持久化边界

- `.proto` 源文件统一位于 `proto/`；`make proto-lint` 使用 Buf 校验 schema。
- `crates/core/protocol/build.rs` 与 `bin/probe-agent/build.rs` 在 Cargo 构建阶段将 Rust
  binding 生成到各自的 `OUT_DIR`。生成 `.rs` 属于 `target/` 构建产物，不进入源码树，
  也不提交到 Git。
- migration 位于 `crates/engines/postgres/src/migrations/`，并必须显式注册到
  `crates/engines/postgres/src/persistence/pool.rs::embedded_migrator()`。
- 主服务 integration test 位于 `bin/molesignal/tests/`。
