---
name: review-rust-code
description: Review MoleSignal Rust diffs, pull requests, or working-tree changes for correctness, regressions, security, performance, architecture, observability, and test coverage. Use for general Rust code review, pre-merge review, regression review, or when the user asks to inspect backend changes without implementing them.
---

# 审查 Rust 代码

执行只读、基于证据的 review。除非用户明确要求修复，否则不要改文件。

## 当前项目背景

- 后端是根 package `molesignal` 的单 crate，源码位于 `src/`。
- 唯一子 crate 是 `src/sqlx-shim`，用于提供受控的 `sqlx` facade。
- 逻辑分层为 `shared`、`domain`、`app`、`infra`、`api`、`bootstrap`。
- integration test 位于根 `tests/`，很多 Docker 测试受 `MS_RUN_IT=1` 控制。

## 审查流程

1. 明确 review 范围和基准，优先检查变更行及其直接调用方。
2. 理解预期行为后再报告问题；不要仅凭关键词猜测。
3. 检查每个发现是否可由输入、状态或调用链实际触发。
4. 只报告开发者会愿意立即修复的具体问题，给出最小定位范围。
5. 不把未触及的历史问题混入本次 review。

## 检查项

- 正确性：边界条件、状态转换、错误映射、取消、重试、幂等和资源释放。
- 类型：使用 `crate::shared::ids::Id`、`TimestampMicros`、`TimeRange` 和领域 enum，避免长期使用 `Result<T, String>`。
- 架构：不新增不必要的反向依赖；repository trait、PG 实现和 wire 装配位置正确。
- 租户：`org_id` / `organization_id` 从入口贯穿 SQL、缓存、对象路径和事件。
- License：商业能力在入口或 worker 周期边界调用 `LicenseGate::has_feature`。
- Async：不持有同步锁跨 `.await`，不在热路径引入阻塞 IO 或无界并发。
- 性能：ingest/query 热路径避免逐条 `format!`、JSON 序列化、大对象 clone 或高频日志。
- 可观测性：使用结构化 `tracing`，不记录 token、密钥、签名包或敏感原文。
- 时间：内部统一微秒，外部协议单位在 `api` 边界转换。
- 产物：proto 生成结果、migration 注册、SPDX 头和必要测试是否同步。
- 文件组织：新增非测试、非生成的生产文件不超过 500 行；实质修改历史超限文件时未继续堆叠独立职责。
- 模块聚合：同一功能拆成多个实现文件时使用专属目录，不在父目录散落同前缀文件，也不创建职责不清的杂物模块。

若 diff 涉及 module boundary、租户隔离、License 或 SQL migration，再应用对应的专项技能，不加载无关专项。

## 输出

按严重程度输出发现：

1. 标题：`[P0-P3]` + 简短问题
2. 精确文件与最小行范围
3. 可触发场景和实际影响
4. 建议修复方向

没有可操作问题时明确说明，并简述未验证的风险或测试缺口。不要用大量格式问题淹没功能问题。
