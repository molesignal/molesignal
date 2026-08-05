---
name: review-module-boundaries
description: Review MoleSignal's single-crate module boundaries, dependency direction, domain ports, infrastructure adapters, protocol conversion, workers, and bootstrap wiring. Use when changes cross src/domain, src/app, src/infra, src/api, src/bootstrap, src/protocol, add repositories or services, or replace the former crate-boundary review.
---

# 审查模块边界

当前后端已从多 crate 合并为单根 crate。把旧“crate 边界审查”解释为逻辑模块边界审查；编译器不再自动阻止反向依赖。

## 目标分层

```text
api -> app -> domain -> shared
 |      ^
 v      |
infra --+

bootstrap -> api/app/infra/domain/shared
```

- `src/shared`：跨模块基础类型、错误、时间、健康检查和 License trait。
- `src/domain`：业务模型与 repository/port trait，不依赖数据库、对象存储、HTTP 或 gRPC adapter。
- `src/app`：用例编排，优先依赖 domain trait 和 shared 类型。
- `src/infra`：PostgreSQL、object store、DataFusion、Tantivy、WAL、通知和外部 adapter 实现。
- `src/api`：axum/tonic 入口、认证授权、DTO 与协议转换。
- `src/protocol`：`buf generate` 产物。
- `src/bootstrap/bootstrap.rs`：唯一总装配入口；同目录功能文件负责分段装配，`roles/` 与
  `workers/` 承载进程角色和后台循环。
- `src/sqlx-shim`：唯一保留的子 crate，不作为新增业务代码容器。

## 检查项

1. `src/domain` 是否新增 `sqlx`、`object_store`、`axum`、`tonic`、`reqwest` 或 `tokio::net` 依赖。
2. 新 repository trait 是否位于相应 `src/domain/**`；PG 实现是否位于 `src/infra/persistence/repositories/`。
3. 新 service 是否在 `src/app/**` 通过 trait 注入能力，而不是直接新增 `PgPool` 耦合。
4. HTTP/gRPC DTO 和生成的 protocol 类型是否尽早转换为领域类型。
5. 新后台循环或长期 `tokio::spawn` 是否由 `src/bootstrap/roles/` 或 `src/bootstrap/workers/` 管理生命周期。
6. `src/bootstrap/bootstrap.rs` 及对应功能装配文件是否实例化并注入了新
   repository、service、worker 和配置。
7. 顶层产品模块 `intelligence`、`license`、`cloud_marketplace`、`domain_management`、`model_pricing`、`report_renderer` 是否复用现有 shared/domain 边界。
8. 是否误用已删除的 `crates/*` 路径、旧 package 名或旧 `protoc` 模块名。
9. 同一功能在某一层拆成多个实现文件时，是否建立对应的专属目录，而不是把文件散落在该层父目录；目录聚合是否仍保持跨层依赖方向。

## 现有例外

当前工作树中 profiling、trace export、self telemetry 等部分 `src/app/**` 模块直接使用 infra、protocol 或网络 adapter。只审查本次 diff 是否扩大耦合；不要把未触及的既有例外当作本次问题。若新增类似例外，要求在 `ARCHITECTURE.md` 记录理由和边界。

当前商业产品模块通常无条件编译，并通过运行时 License gate 控制；不要沿用旧文档中的空 `cfg=` 或要求不存在的商业 feature crate。

## 输出

1. 总体结论
2. 新增反向依赖或类型穿透（file:line）
3. trait/adapter 放置问题
4. worker 生命周期或 bootstrap 装配缺漏
5. 推荐的最小重构
