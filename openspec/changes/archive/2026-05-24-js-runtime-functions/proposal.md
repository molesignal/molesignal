## Why

`Function` 实体已声明 language ∈ {Vrl, Js}，VRL runtime 已接入 pipeline（`functions-runtime` change 已交付）。但 language=Js 的 function 当前 100% 报错 `javascript runtime not yet implemented (build with feature=js)`。OO 用户提的最频繁的 ingest-time transform 需求是"用 JS 写处理逻辑"，缺这个会被竞品拉开差距。

## What Changes

- 引入 `deno_core` 作为 javascript runtime 后端（cargo feature `js-runtime` 默认关闭）。
- 新增 `JsFunctionExecutor`（infra 层，与 `VrlFunctionExecutor` 同级），impl `app::ingestion::FunctionExecutor`。
- 每个 function 一个 isolate（V8 isolate 隔离）；编译结果缓存按 `(function_id, updated_at)`。
- 限额：每次 `run` 50ms wall clock + 32 MiB heap；超时 / OOM → 跳过 event 并写 `IngestError`。
- runtime 内暴露最小 `globalThis.molesignal = { now, log, fields, set, delete: del, hash_sha256, parse_json, encode_json }`，让用户不需要 `import` 任何东西；event mutate 通过 `molesignal.set("field", value)` 或直接改 `molesignal.fields` 对象。
- wire 阶段 `cfg(feature="js-runtime")` 时把 executor 升级成 chain：`if function.language == Js → JsExecutor else → VrlExecutor`；feature off 时维持现状（Js function 报 invalid）。
- `functions.precheck_compile` 在 feature on 时也试编译 JS（捕获 syntax error 提前 fail）。

## Capabilities

### New Capabilities
<!-- 无 -->

### Modified Capabilities
- `functions-runtime`: 把 JS function 从"占位 reject"升级到真实可执行。

## Impact

- **依赖**：workspace 加 `deno_core = "0.330"`（~30 transitive deps；故 feature-gated）。
- **新文件**：`crates/infra/src/runtime/js_executor.rs`（`cfg(feature="js-runtime")`）。
- **wire**：`function_executor` 构造时 feature-gate 注入 `ChainedExecutor { vrl, js }`。
- **配置**：`[functions]` 加 `js_runtime_enabled`（默认 false，意为"即使编译时开了 js-runtime feature，runtime 仍可一键关闭"）。
- **测试**：4 个 unit test —— set 字段、del 字段、超时、syntax error 提前 fail。
- **风险**：编译时间从 ~1.5 分钟提到 ~5 分钟（deno_core build heavy）；故默认 OFF。
- **OSS 影响**：default features 下 OSS build 时长不变；ops 用 `--features js-runtime` 切到 v8 路径。
