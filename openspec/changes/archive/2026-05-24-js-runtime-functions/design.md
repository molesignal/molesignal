## Context

Functions runtime VRL path 完整跑通（`functions-runtime` change），JS 路径仅有 schema + 拒收。两种主流后端：

- `deno_core`：Deno 团队维护，纯 V8，社区生态最大，编译时间长
- `boa_engine`：纯 Rust 实现的 ES2017 子集，编译快，但执行慢且不支持现代 JS 语法
- `quickjs-wasm-rs`：QuickJS wrapped in WASM，资源占用小但调试与 stack trace 体验差
- `rusty_v8`：直接调 V8 C++ 接口，自己接 sandboxing，工作量爆炸

ingest 路径每秒万事件 + 每条要执行 JS，跑得快比编译快重要。`deno_core` 是 V8，性能在 ms 内每条；执行慢用户立刻拉警告。

## Goals / Non-Goals

**Goals:**
- 用户能写常见 transform JS（field 改名、字符串处理、JSON 解析、字段 lookup）
- 每条 event 严格资源限额（wall + heap），失败 isolate 不污染下一条
- Feature off 默认（编译时 + 运行时两道开关），避免普通用户被拖编译

**Non-Goals:**
- 不暴露 `fetch` / `import` / file IO（沙箱内 deno_core 不带 deno_runtime；意图就是限制能力）
- 不支持 async function 的 await（pipeline 是同步行为；JS 主线程跑完）
- 不支持 npm / module imports（单文件 source 即可）
- 不实装 source map debug（用户拿到 line / col 错误已够）

## Decisions

### D1：选 deno_core，feature-gate

deno_core 编译时间长且二进制大（V8 ~20MB），但是它给的执行速度 + ECMAScript 兼容度 + 错误信息质量都领先。默认关 feature，意愿用 JS 的 ops 自己开。

### D2：每 function 一个 isolate？还是 per-batch？

per-event 一个 isolate 太慢（V8 启动几 ms）；per-batch 一个 isolate 会有 state leak（前 event 改了 globalThis 会影响后 event）。

折中：per-function 一个 isolate（与 compile cache 一致），每次 run 前 reset `molesignal.fields`，运行后清空 globalThis 上用户加的 key。

### D3：限额怎么实现

deno_core 提供 `v8::IsolateHandle::terminate_execution` —— 在另一个 task 上 50ms timer 触发 terminate。heap limit 走 `v8::Isolate::set_oom_error_handler` 在 32 MiB 时 abort。

### D4：暴露面控制

只暴露 `molesignal` global 与 `console.log` 重定向到 tracing。`globalThis.crypto` / `setTimeout` / `Promise` / `fetch` 等都不挂。 deno_core 默认不带 `deno_runtime`，所以默认就没有这些，符合最小特权。

### D5：event mutate via `fields` object

JS 端拿到的 `molesignal.fields` 是真实 event 的代理。修改它直接反映到 Rust 侧 `serde_json::Value`。这通过 deno_core 的 `op` + 自定义 ObjectTemplate 实现，复杂度略高，但是用户体验远好于"返回新 object"。

替代方案：用户必须 `return { ... }`。拒：用户老忘 return，错误率高。

## Risks / Trade-offs

**[R1] 编译时间膨胀 ~5min**
→ Mitigation：feature off 默认，CI 仅在 release / nightly 跑 JS 路径。

**[R2] V8 漏洞 = SaaS 风险**
→ Mitigation：定期 cargo update；deno_core minor 跟 V8 patch；公告页注明 hosted 用户需要 ops 自己更新二进制。

**[R3] 跨 event isolate 状态污染**
→ Mitigation：每次 run 前清空 globalThis 上非 frozen key；测试 cover：第一次 set globalThis.x，第二次不该看到。

**[R4] terminate_execution 不立即生效**
→ Mitigation：deno_core 在 `v8::Locker` 释放点才能 terminate；纯计算无 await 时也能在下次 backedge 触发。50ms 超时 + 200ms 硬等再 force-kill isolate（重建）。
