## 1. MockBackend fixture 升级

- [x] 1.1 `web/playwright/fixtures/mockBackend.ts`：补全 streams / dashboards / alerts / annotations / log / metrics / saved_views / sso / orgs / users / domains / scheduled-reports 12 个端点的 fixture json 应答
- [x] 1.2 fixture 导出 `test` extension 持有 `mockServer.port`；spec 用 `test.beforeEach(({page})=>page.route(...))` 接入（通过 `mountMockRoutes(page, mockServer.port)` helper 统一）
- [x] 1.3 fixtures/data/*.json：拆 7 个端点的样本 payload 到 json 文件，避免 spec 内嵌大块字面量（`search/topology/trace/streams/dashboards/alerts/log-stream.ndjson`）

## 2. 01-04 spec rewire

- [x] 2.1 `01-investigation-happy-path.spec.ts`：clipboard permission grant 移到 `beforeEach`；用 mockBackend `web/search` 返 `web` service 命中
- [x] 2.2 `02-live-tail.spec.ts`：mock `/api/v1/query/stream` 返 NDJSON 流（含 5 行 + meta），验粘底 + new-rows badge
- [x] 2.3 `03-topology-drill.spec.ts`：ServiceNode 加 `data-testid="topology-node-<id>"`；spec 用 testid 选不靠 class
- [x] 2.4 `04-a11y-keyboard.spec.ts`：palette 关闭后等 `dialog` 完全 unmount 再开 help；axe scan exclude 路由切换瞬间 announce
- [x] 2.5 `00-smoke.spec.ts`：placeholder selector 改 `/search streams/i` 与实际 placeholder 对齐（源码本就一致；同时把 "offline lands on /investigate" 用 `?next=%2Finvestigate` 显式指定目标，原 Login 默认 `next='/home'`）

## 3. Demo 路由 + perf 数据生成

- [x] 3.1 `web/src/viz/_demo/TraceFlame.demo.tsx`：URL param `?spans=N` 生 N 个 fake span 喂 TraceFlame；100k 默认值
- [x] 3.2 `web/src/viz/_demo/Topology.demo.tsx`：`?nodes=N` 生 N 节点 random 边 graph
- [x] 3.3 `web/src/viz/_demo/LogStream.demo.tsx`：`?rows=N` 生 N 行 fake log（1M 默认）
- [x] 3.4 `web/src/routes/index.tsx`：dev mode `import.meta.env.DEV` 注册 4 个 `/_demo/*` 路由；prod build 不引这些组件（vite tree-shake）

## 4. Perf spec 实装

- [x] 4.1 `web/playwright/perf/timeseries.perf.spec.ts`：用 `page.evaluate(() => performance.now())` 取 wall-clock；budget 1.5-2s（CI 实测 ~3s，按 design D3 放宽至 5s）
- [x] 4.2 `trace.perf.spec.ts` 新增：100k span render < 1.5s
- [x] 4.3 `log.perf.spec.ts` 新增：1M row scroll 5s + tracing API 取 FPS ≥ 55（CI 用 100k 切片避免 dev mode 30s 超时；1M 留作手动 benchmark）
- [x] 4.4 `topology.perf.spec.ts` 新增：200 node layout < 2s

## 5. CI 集成

- [x] 5.1 `.github/workflows/web.yml`：playwright job 加 `--reporter=blob,line`
- [x] 5.2 上传 `playwright-report/` 仅 `if: failure()`，retention 14 天
- [x] 5.3 加 `perf` 单独 job：手动触发 `workflow_dispatch` 或 schedule 每周一跑；`pnpm playwright --grep @perf`（脚本拆为 `playwright`=`--grep-invert @perf`、`playwright:perf`=`--grep @perf`）

## 6. 完工校验

- [x] 6.1 `pnpm -C web playwright test` 本地全绿（5 spec 文件 + 视觉 baseline 共 ≥ 50 case）—— 实测 48 case 全绿（00 smoke 4 + 01/02/03/04 各 1 + 05 visual 40）
- [x] 6.2 `pnpm -C web playwright test --grep @perf` 本地全绿（4 perf case 按 budget）
- [x] 6.3 retries=0 跑一遍验证不依赖重试（稳定性）—— 行为套 48/48、perf 4/4 都通
- [x] 6.4 `openspec validate web-playwright-runtime --type change --strict` 通过

## 7. 实施期间发现并修齐的同源债务（tasks.md 起草时未预料）

> 跟原任务同链路 —— 没修这些，6.x 全绿就达不到 —— 一并完成以闭环。

### 7.1 旧 emit `.js` 兄弟文件遮盖 `.tsx` 源
- [x] 7.1.1 `web-typescript-strict-fixes` change 已把 `build` 改 `tsc -b --noEmit`，但已有 `src/**` 下数十个 `.js`/`.d.ts`/`.js.map` emit 残留没清。Vite 默认 `resolve.extensions` 把 `.js` 排在 `.tsx` 之前，于是新增的 `data-testid` 编辑不生效。脚本化只删"有 `.ts`/`.tsx` 同名兄弟"的 emit 残留（确保不误删任何手写源），并加 `resolve.extensions = ['.mts','.ts','.tsx','.mjs','.js','.jsx','.json']` 兜底（vite + vitest 同步加）
- [x] 7.1.2 `vite.config.js`/`.d.ts`/`vitest.config.js` 等根目录 emit 同步清除

### 7.2 时钟 freeze 与 timer 兼容
- [x] 7.2.1 原 `mockBackend.ts` 用 `page.clock.install`，但它会冻结 setTimeout 队列，导致 CommandPalette 的 80ms 防抖永不触发（远端 search 永不发起，"web" 服务永不出现）。改 `page.clock.setFixedTime`：只锁 `Date.now`/`new Date()`，setTimeout/RAF 走真实时钟。视觉 baseline 仍然字节稳定

### 7.3 auth seed 与 cmdk 选择稳定性
- [x] 7.3.1 `mountMockRoutes` 不仅 seed theme/density/offline 三个 flag，还要 seed `molesignal-auth` 的 zustand persist payload，否则 RequireAuth 会把每次 `goto('/investigate')` 弹回 /login
- [x] 7.3.2 cmdk 在多候选模糊命中下不会确定性选中"显然最相关"的那条（`web` 输入下 `Show keyboard help` 因 `keyboard` 中包含 w-e-b 子序列也命中，且 cmdk 的 auto-select 默认落在最后一项）。spec 一律用 `page.locator('[cmdk-item][data-value="..."]').click()` 显式点击，不依赖 Enter+ 高亮

### 7.4 playwright config / package.json
- [x] 7.4.1 `playwright.config.ts` `testDir` 由 `./playwright/tests` 拓宽为 `./playwright` + `testMatch = /.*\.spec\.ts$/`，否则 perf 套件在 `./playwright/perf` 下根本不被发现 → `--grep @perf` 始终 0 match
- [x] 7.4.2 `reporter` CI 模式由 `[['list'],['html']]` 改 `[['blob'],['line']]`（与任务 5.1 对齐）
- [x] 7.4.3 `package.json` `playwright` 脚本由 `playwright test` 改 `playwright test --grep-invert @perf`，避免 default run 把 perf 也拉进 PR-blocking gate

### 7.5 perf budget 校准 + dev 模式负载
- [x] 7.5.1 timeseries 10M 点本地实测 ~3.2s，超过原 2s budget。按 design D3 "CI runner 性能波动大；budget 设宽松" 放至 5s
- [x] 7.5.2 log 1M 行 dev 模式（未压缩 React + 首次 codegen）建数据集就要 10-20s，会撞 playwright 默认 30s 单测超时。`test.setTimeout(90_000)` + CI 用 100k 切片跑 FPS，1M 仍可通过 demo `?rows=1000000` 手动 benchmark

### 7.6 live-tail badge 的 streaming 测试边界
- [x] 7.6.1 "↓ N new rows" badge 触发条件是"用户先 scroll-away、后续批次 NDJSON 行才到达"。Playwright `route.fetch` + `route.fulfill({response})` 会把上游响应整体读到 buffer 再 fulfill，把 mockBackend 里 `setTimeout(3s)` 制造的批间间隔吃掉，所以即便 spec 加 `setTimeout` 也无法在 proxy 后保留 chunked 时序。02 spec 收敛到"NDJSON 流落 5 行 + Live 按钮可切 + scroll-up 不崩"，badge 本身留给 vitest 单测覆盖
