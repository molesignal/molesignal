## Context

Enterprise `actions` crate（`ActionExecutor` + `WebhookClient` + `render_template`）已就位，OSS `action_executions` 表 + `ActionRepository::record_execution` 已就位（spec M1）。但 alert escalation 还不会触发 actions —— `EscalationDispatcher::tick` 当前只走 `User` / `Schedule` / `Team` 三类 `EscalationTarget`。

本 change 把"alert 升级 → 触发 action"这一跳打通。

## Goals / Non-Goals

**Goals:**
- 用户在 `EscalationPolicy.steps[].targets` 里能写 `{kind: "action", action_id}`，escalation 触发时 action 跑起来。
- OSS build 编译可过（`Action` 变体存在），运行期遇到 `Action` target 安全 skip + warn。
- Enterprise build + license 时实际跑 webhook，结果落 `action_executions`。
- 单测覆盖 4 个分支：serde 往返 / OSS skip / license skip / 真跑 + record_execution。

**Non-Goals:**
- 不实装 script kind 的真实 sandbox 运行（enterprise crate 已是 stub）。
- 不改 HTTP `escalation_policies` 路由的请求/响应类型字段 —— `targets` 字段已是 `serde_json::Value`，新变体经 untagged enum 兼容老 payload。
- 不引入新表（`action_executions` 表已建）。

## Decisions

### D1：`EscalationTarget` 新变体放在 domain 层

把 `Action { action_id: Id }` 加在 `crates/domain/src/alerting/escalation.rs::EscalationTarget`。理由：
- 保持 escalation policy 的整体语义在 domain 完整定义（与 user/schedule/team 同级）；
- Pg `escalation_policies.steps` 是 JSONB，serde tag = "kind" 已就位，加一个 variant 不破坏老行 read。
- 替代方案：在 enterprise crate 外挂、走 metadata bag —— 拒，理由：把核心枚举打散到 enterprise 会让 OSS dispatcher 无法 match arm，触发隐式 fallthrough。

### D2：`ActionExecutorPort` trait 在 app 层

新增 `app::alerting::ActionExecutorPort` trait（OSS noop 实装 + enterprise adapter 实装）。理由：
- app crate 不能依赖 infra / enterprise（架构约束）；
- dispatcher 拿 `Arc<dyn ActionExecutorPort>`，wire 阶段切换 impl；
- 类似 `FunctionExecutor` 的设计（`crates/app/src/ingestion/pipeline.rs`），已有同种模式。

### D3：OSS noop 行为 = Skipped + warn

OSS 编译时（或 enterprise 编译但无 license）拿到 `Action` target → `ExecutionResult { status: Skipped, error: Some("actions feature not licensed") }`，dispatcher warn 日志 + 不阻塞 sibling targets / step 推进。

替代方案：编译期 `cfg=enterprise` 直接不让 `Action` variant 存在 —— 拒，破坏 OSS / 企业版二进制 schema 兼容（同一 escalation_policies row 在两种二进制都能 read）。

### D4：每次 action 调用写一条 `action_executions`

包括 skipped 也写。理由：用户在前端 `actions/<id>/executions` 能看到"为啥这个 webhook 没跑"，否则会以为 dispatcher 卡了。

## Risks / Trade-offs

**[R1] `EscalationTarget` 增 variant 破坏所有 match arm**
→ Mitigation：编译期 rustc 报全部 `non_exhaustive` 错；用一个 grep + cargo check 双保险确认覆盖。涉及位置：dispatcher.rs / serde tests / Pg `escalation_policies` 行解码（已是 untagged 经 serde_json::Value，无需改）。

**[R2] action 跑慢拖延整个 step**
→ Mitigation：`ActionExecutor::execute` 已用 `tokio::time::timeout`（10s）；dispatcher 收 ExecutionResult 后立即返回，不串行等待第二个 target。

**[R3] license 状态在 tick 中变化**
→ Mitigation：每个 tick 都重新读 `license.has_feature(...)`，license 一旦失效下次 tick 立即停跑（无需 hot reload）。
