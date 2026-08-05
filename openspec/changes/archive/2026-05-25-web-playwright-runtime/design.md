## Context

`web-investigation-shell` 落地了 `playwright.config.ts`、4 个 spec 文件、1 个 perf 套件文件、1 个 mockBackend fixture，但只有视觉回归 baseline（`05-visual.spec.ts`）跑通了 40/40。其他 4 个 spec 文件失败原因（来自归档前实跑）：

- 00 smoke：locator 模糊匹配失败（`/search streams/i` vs `/search/i`）
- 01 happy path：clipboard permission grant 时机晚于 `y` 按键
- 02 live-tail：SSE 端点不存在，挂在 connect 阶段
- 03 topology drill：ReactFlow node 选不到（class 名变了）
- 04 a11y：`getByRole('dialog')` 在 ⌘K 已关后再开 `?` 时 race

Perf 套件全部没写过实际 demo 路由对应代码，目前是 placeholder。

## Goals / Non-Goals

**Goals:**
- `pnpm -C web playwright test` 全绿（含 05 视觉 + 00-04 行为 + smoke 4 个）
- `pnpm -C web playwright test --grep @perf` 4 个 perf case 全绿（按 budget assert）
- CI playwright job 不 flake（retries=2 内必稳）
- 不依赖任何 dev backend；任何 `/api/v1/*` 调用都被 mock 拦
- trace artefact 自动上传（已在 yml；本 change 验证）

**Non-Goals:**
- 不实装 dev server hot-reload guard
- 不接 cypress / TestCafe（仅 Playwright）
- 不补 cross-browser（仅 chromium）
- 不写 mobile viewport（最小 1280px 来自 web-shell spec）

## Decisions

### D1：mockBackend 用 Express in-test，不用 MSW

跟 web-investigation-shell 决策保持一致（MSW 在 node 跑 service worker 模式吃不消；Express 起本地随机端口 + `page.route(/api/v1/**)` 转发是直接路径）。

### D2：4 个 demo 路由打补丁路由表，仅 dev 启用

`vite.config.ts` 已分 `mode === 'development'` 时 enable demo；本 change 让 `/_demo/*` 路由在 router 内做 conditional 注册，避免 prod build 出 demo 代码。

### D3：Perf budget = wall-clock，不精算

CI runner 性能波动大；budget 设宽松（10MP ts paint < 2s 而非 spec 上的 500ms），重在"不退化"而非"达 spec"。spec 上原始 budget 留在 `// EXPECTED: <num>ms` 注释里做后续优化目标。

### D4：trace artefact 上传走 `actions/upload-artifact@v4`

已有；本 change 加 `--reporter=blob,line` 让 trace 跟 HTML report 都生成。失败时只上传 trace 节省 CI 流量。

### D5：所有 spec 用 `test.beforeEach` 注入 mock + clock

跟 `05-visual.spec.ts` 的 pattern 完全对齐：clock 冻 `2026-05-23T10:00:00Z`、localStorage seed theme/density/offline、`page.route('**\/api/v1/**', ...)` 全拦。这样 4 套 spec 跟 visual 一份 fixture，维护成本统一。

## Risks / Trade-offs

**[R1] Express fixture 在 Playwright worker 内端口冲突**
→ Mitigation：`listen(0)` 随机端口；fixture 拿到端口后 `page.route` 转发，避免每个 worker 抢同一端口。

**[R2] Demo 路由数据生成耗时**
→ Mitigation：100k span / 1M row 用 `crypto.getRandomValues` + 一次性 Float64Array 在 mount 时建；不算 perf budget 一部分。

**[R3] ReactFlow class 名跟版本走**
→ Mitigation：用 `data-testid="topology-node-<id>"` 在 ServiceNode 里加 testid，spec 用 testid 选不靠 class。

**[R4] @perf grep 把 visual spec 也匹了**
→ Mitigation：把 perf spec name 改 `[@perf] xxx`，visual 用 `visual @ theme=...`，grep `@perf` 不会撞上 visual（"@ theme" 不含 "@perf"）。
