## Why

`web-investigation-shell` 在归档清单里留了 4 套 e2e spec（01 happy path / 02 live-tail / 03 topology drill / 04 a11y keyboard）+ 1 套 perf（15.3）作为 deferred items：spec 文件已写，但都没在 dev env 实跑过——因为它们假设有一个"真后端"在 `localhost:5173/api/v1/*` 应答。

15.2 / 15.3 要 land，必须给 e2e 套件配一个**确定性 mock backend**（在 fixture 内启）+ 让 dev server 启动稳定 + 让 4 套 spec 真在 Playwright 跑过且不 flaky；perf 套件需要造 demo 路由 + 性能预算 assertion 真在 trace metrics 上比对。

本 change 做这一层，承接已经被 `web-investigation-shell` 14.x 落了的 visual 40 张 baseline（稳定的 mockBackend 模式可复用）。

## What Changes

- **MockBackend 全套**：把现有 `playwright/fixtures/mockBackend.ts` 升级成所有 `/api/v1/*` 端点都有可控 fixture json 应答，时间冻在 `2026-05-23T10:00:00Z`（沿用 visual baseline）；导出 `test`（base+fixture）让 01-04 spec 切到带 mock 的 base。
- **01-04 spec rewire**：4 个 spec 文件从 `@playwright/test` 切到 `fixtures/mockBackend`；fix 现实 selector 不匹配（如 happy path 里 `getByPlaceholder(/search/i)` 实际是 `/search streams/i`）；移除 hard-coded URL 假设。
- **Perf 套件**：写 `web/src/viz/_demo/{TraceFlame,Topology,LogStream}.demo.tsx` 3 个新 demo 路由（已有 TimeSeries demo），喂 100k span / 200 node / 1M row 合成数据；perf spec 用 `page.evaluate(performance.now())` 取真实 measure，对照 budget。
- **CI 集成**：`.github/workflows/web.yml` playwright job 已有；加 `--reporter=blob,line` + 失败上传 trace；perf 套件单独 job，仅 `--grep @perf`。
- **失败补救**：00-smoke.spec.ts 里 `/continue offline/i` 在某些主题下 ipad 视窗失败 —— 本 change 顺手 fix selector。
- 不重写视觉 baseline（40 张已稳定）；不动 vitest。

## Capabilities

### New Capabilities
<!-- 无 -->

### Modified Capabilities
- `web-shell`: 增"Playwright runtime gate"（typecheck 后 e2e + perf 必须绿）

## Impact

- **新 demo 路由**：`/_demo/timeseries` `/_demo/trace` `/_demo/log` `/_demo/topology`（仅 dev 启用，build 不打）
- **playwright fixtures**：rewrite mockBackend；fixtures/dir 加 trace/log fixture json
- **CI**：playwright job 强制 mock-based；perf 单独 grep
- **风险**：极低；本 change 不动产品行为，仅测试基础设施
- **跟随**：land 后 `web-a11y-baseline` / `web-ui-polish` 才有稳定 e2e 跑得通
