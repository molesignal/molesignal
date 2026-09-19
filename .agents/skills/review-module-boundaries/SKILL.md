---
name: review-module-boundaries
description: Review MoleSignal's workspace crate and module boundaries, dependency direction, domain ports, infrastructure adapters, protocol conversion, workers, and bootstrap wiring. Use when changes cross bin/molesignal or crates/core, crates/engines, crates/modules, crates/transport, crates/support, or add repositories and services.
---

# 审查模块边界

当前后端使用 virtual workspace。既审查 Cargo crate 依赖，也审查 `bin/molesignal`
内部尚未提取部分的逻辑分层。

## 目标分层

```text
bin/molesignal -> modules/engines/transport/support/core
modules -------> core/support
engines -------> core/support
bootstrap -----> api/app/infra + workspace crates
```

- `crates/core/domain`：业务模型与 repository/port trait，不依赖 adapter。
- `crates/engines/postgres`：PostgreSQL repository、migration、持久化加密与 Probe CA。
- `crates/modules`：可独立演进的产品能力；不依赖应用 composition root。
- `crates/transport` 与 `crates/support`：协议客户端和跨域技术设施。
- `bin/molesignal/src/app`：用例编排，优先依赖 domain trait。
- `bin/molesignal/src/infra`：尚未提取的 object store、DataFusion、Tantivy、WAL、通知等 adapter。
- `bin/molesignal/src/api`：axum/tonic 入口、认证授权、DTO 与协议转换。
- `crates/core/protocol`：协议 module；`build.rs` 将 prost/tonic binding 生成到 Cargo
  `OUT_DIR`，生成文件不进入源码树。
- `bin/molesignal/src/bootstrap/bootstrap.rs`：总装配入口；同目录功能文件负责分段装配，`roles/` 与
  `workers/` 承载进程角色和后台循环。
- `crates/support/sqlx-shim`：受控 `sqlx` facade，不作为业务代码容器。

## 检查项

1. `crates/core/domain` 是否新增 `sqlx`、`object_store`、`axum`、`tonic`、`reqwest` 或 `tokio::net` 依赖。
2. 新 repository trait 是否位于相应 domain；PG 实现是否位于 `crates/engines/postgres/src/persistence/repositories/`。
3. 新 service 是否通过 trait 注入能力，而不是直接新增 `PgPool` 耦合。
4. HTTP/gRPC DTO 和生成的 protocol 类型是否尽早转换为领域类型。
5. 新后台循环或长期 `tokio::spawn` 是否由 `bin/molesignal/src/bootstrap/roles/` 或 `workers/` 管理生命周期。
6. `bin/molesignal/src/bootstrap/bootstrap.rs` 及对应功能装配文件是否实例化并注入了新
   repository、service、worker 和配置。
7. `crates/modules/*` 是否复用 core/domain 边界，且没有反向依赖 `molesignal`。
8. 新单一职责 crate 是否放入正确分类目录并去掉 `molesignal-` 前缀。
9. 同一功能在某一层拆成多个实现文件时，是否建立对应的专属目录，而不是把文件散落在该层父目录；目录聚合是否仍保持跨层依赖方向。

## 现有例外

当前工作树中 profiling、trace export、self telemetry 等部分
`bin/molesignal/src/app/**` 模块直接使用 adapter、protocol 或网络客户端。只审查
本次 diff 是否扩大耦合；不要把未触及的既有例外当作本次问题。

当前商业产品模块通常无条件编译，并通过运行时 License gate 控制；不要沿用旧文档中的空 `cfg=` 或要求不存在的商业 feature crate。

## 输出

1. 总体结论
2. 新增反向依赖或类型穿透（file:line）
3. trait/adapter 放置问题
4. worker 生命周期或 bootstrap 装配缺漏
5. 推荐的最小重构
