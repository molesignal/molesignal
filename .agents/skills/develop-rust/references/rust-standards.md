# Rust 编码规范

## 基础原则

- 使用类型表达业务语义，复用 `crate::shared::ids::Id`、`TimestampMicros`、`TimeRange`、`StreamType` 等现有类型。
- 默认将组织隔离视为硬约束；repository、service、缓存、对象路径和事件携带 `org_id` 或 `organization_id`。
- 使用 `crate::shared::{Error, Result}`，不要把 `Result<T, String>` 作为长期接口。
- handler/service/repository 不因外部输入 `panic!`；优先返回带语义的 `Error`。
- 外部协议的解析、认证与时间单位归一尽量在 `src/api` 边界完成。
- 先查找并复用现有抽象，不创建同义的第二套领域类型或 repository。

## 文件长度与模块组织

- 除生成代码和测试代码外，新建生产实现文件不得超过 500 行。生产源码中的 `#[cfg(test)]`、`mod tests` 等测试块不计入该上限。
- 已有生产文件超过 500 行时，不得继续加入新的独立职责。若当前任务实质修改该文件，应把本次涉及的内聚逻辑拆出；极小且孤立的修复无需重构无关历史代码，但不得进一步恶化结构。
- 按职责、领域概念或执行阶段拆分，不能机械按行数切割，也不能靠压缩格式、合并语句或删除必要注释规避上限。
- 同一功能或模块需要两个及以上实现文件时，新建专属目录集中放置。Rust 可按仓库现有风格使用 `feature.rs` + `feature/*.rs`，或 `feature/mod.rs` + 子文件。
- 不把一组 `feature_*.rs` 散落在父目录中，也不把拆出的代码塞进职责不清的 `utils`、`helpers`、`common` 或 `misc`。
- 目录内仍须保持逻辑分层。跨 `api`、`app`、`domain`、`infra` 的同一功能，应分别归入各层对应的功能目录，不能为了物理聚合破坏依赖方向。
- 生成代码必须有 `@generated`、`DO NOT EDIT` 等明确标记，或能由项目生成流程确定；测试豁免适用于独立测试文件、测试目录和生产源码内的测试模块。

## 领域类型

`Id` 内部是字符串，可承载 KSUID 或 UUID 文本；数据库通常使用 `TEXT` 或 `VARCHAR(64)`。不要仅凭类型名假定 PostgreSQL 列必须是 `UUID`。

优先使用：

```rust
use crate::{
    domain::stream::StreamType,
    shared::{
        Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};
```

HTTP/gRPC 可以接收字符串，但应尽早解析成领域 enum 或 ID。

## 错误处理

使用已有构造器：

- `Error::not_found`
- `Error::invalid`
- `Error::conflict`
- `Error::unauthorized`
- `Error::forbidden`
- `Error::payment_required`
- `Error::resource_exhausted`
- `Error::payload_too_large`
- `Error::unavailable`
- `Error::internal`

数据库错误通过 `src/infra/persistence/mod.rs::sqlx_err` 统一处理常见 `RowNotFound` 与 SQLSTATE `23505`，或在确需特殊语义时显式映射。

不要把内部错误文本、SQL、token、license 签名包或密钥返回给客户端。`Error` 的 5xx 响应会隐藏内部详情。

## Option、Result 与不变量

- `Option` 表示值可能不存在。
- `Result` 表示操作可能失败且失败有原因。
- License 或权限失败返回明确错误，不用 `None` 或空成功响应吞掉。
- `unwrap()`、`expect()`、`panic!()` 只用于测试或外部输入无法触发的内部不变量；`expect` 信息要解释不变量。

## 所有权

- 只读优先借用，修改使用 `&mut`。
- 只有跨 task/channel 或确需独立 ownership 时 clone 大对象。
- `Arc<dyn Trait>` 的 `Arc::clone` 可以用于依赖共享，不要把它与深拷贝混淆。
- 避免在 ingest/query 热路径复制完整 event、record batch、JSON 或 buffer。

## Async 与并发

- 不持有 `std::sync::Mutex` 或 `parking_lot` guard 跨 `.await`。
- 需要跨 await 的互斥使用 `tokio::sync`，或重新划分临界区。
- 不在 async handler 中执行无界阻塞 IO；必要时使用受控的 blocking pool。
- 新建 background loop 时提供取消、退出、错误退避和 bootstrap 生命周期管理。
- 避免无界 `spawn`、channel、集合和重试。

## 日志与敏感数据

使用结构化 `tracing`：

```rust
tracing::info!(
    org_id = %org_id,
    rule_id = %rule.id,
    matched,
    "alert rule evaluated"
);
```

- `%` 用于 `Display`，`?` 用于受控的 `Debug`。
- 不在 ingest/query 热路径逐条记录 debug/info。
- 不记录 access token、API key、密码、cookie、license signed package、模型 secret 或完整敏感 payload。
- 删除临时 `println!`、`eprintln!` 与 `dbg!`。

## 时间

内部时间统一使用微秒：

- 类型：`TimestampMicros`
- 数值字段：`*_micros: i64`
- 区间：`TimeRange`

OTLP nanoseconds 等外部单位在入口转换。不要新增单位不明的 `timestamp: i64` 或 `time: i64`。

## 多租户

- 从可信认证上下文取得组织 ID，不信任 body、query、模型输出或 webhook 自报值。
- SQL 的 SELECT/UPDATE/DELETE/UPSERT 都要保留组织谓词。
- cache key、对象路径、job key 和 broadcast topic 带组织维度。
- system scope、platform administrator、public share 和 cross-org grant 必须有显式权限路径。

## License

受限功能在入口或 worker 周期边界调用：

```rust
if !state.platform.license.has_feature(FEATURE) {
    return Err(Error::forbidden(format!(
        "{FEATURE} feature not licensed"
    )));
}
```

不要缓存可能在 `LicenseHolder` 替换后失效的授权结果。社区版必须继续关闭商业 feature。

## Protocol 与序列化

- HTTP：Serde JSON。
- gRPC/内部协议：prost/tonic，生成代码位于 `src/protocol/`。
- 配置：TOML/Figment。
- 列式数据：Arrow/Parquet，复用现有 schema 转换。
- hot path 避免重复 `serde_json::to_string` 或动态 `format!`。
- 不在生成的 protocol 类型上实现业务逻辑。

## SPDX

新增自有 `.rs` 文件首部：

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors
```

`src/sqlx-shim` 保留上游版权，脚本会排除该目录；生成文件带 `@generated` 或 `DO NOT EDIT` 时也会跳过。

## 注释与文档

- 注释解释原因、约束或不变量，不逐句翻译代码。
- 公共 API 使用 `///`；重要模块使用 `//!` 说明职责与取舍。
- 改变架构、配置、运行方式或公共行为时同步更新相应文档。

## 测试

- 小范围逻辑优先源码内 unit test。
- 跨模块行为放在根 `tests/`。
- 优先 in-memory fake；真实 PostgreSQL 路径使用 testcontainers。
- 测试名描述行为和预期结果。
- 修复 bug 时补能在修复前失败的回归测试。

## Unsafe

默认禁止新增 `unsafe`。确需使用时：

- 缩小 unsafe block。
- 写 `// SAFETY:` 说明不变量。
- 覆盖边界、生命周期与错误路径测试。
