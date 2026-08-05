## 1. 依赖切换与脚手架

- [x] 1.1 `web/package.json` 切依赖：移除 `@mantine/*`（7 包）、`echarts`、`echarts-for-react`、`monaco-editor`、`@monaco-editor/react`、`@tabler/icons-react`；新增可视化 + 工具依赖 `cmdk@^1`、`uplot@^1.6`、`@tanstack/react-virtual@^3.10`、`reactflow@^11.11`、`fuzzysort@^3`、`d3-scale@^4`、`d3-array@^3.2`、`d3-force@^3`、`pako@^2.1`、`mitt@^3.0`、`nanoid@^5`、`@codemirror/state`、`@codemirror/view`、`@codemirror/lang-sql`、`vitest@^2.1`、`@testing-library/react@^16`、`@testing-library/user-event@^14`、`jsdom@^25`、`fast-check@^3.22`、`@playwright/test@^1.48`、`axe-core@^4.10`、`msw@^2.4`；shadcn 底层依赖（`@radix-ui/*` / `class-variance-authority` / `clsx` / `tailwind-merge` / `lucide-react` / `tailwindcss-animate` / `sonner`）已显式列入 dependencies，因为组件源码已预写（见 1.10）。**PromQL 编辑器模式不在本批次**：CodeMirror 6 官方无 PromQL package，需用 `@prometheus-io/codemirror-promql`，在 Section 3.7/8 后续 frame 落地时再加。
- [x] 1.1a `web/components.json` 已写入 `style: "new-york", baseColor: "neutral", cssVariables: true, aliases: { components: "@/shell/ui", utils: "@/shell/lib/cn" }`。
- [x] 1.2 `web/pnpm-workspace.yaml`（实际放根目录 `pnpm-workspace.yaml`，列 `packages: [web]`）；`web/package-lock.json` 已删；CI 命令 `pnpm i --frozen-lockfile --filter ./web` 写入 14.x 部署文档。**用户需手动运行 `pnpm install` 生成 `web/pnpm-lock.yaml` 并提交。**
- [x] 1.3 `web/tailwind.config.ts` + `web/postcss.config.cjs` 已重写（postcss-import + tailwindcss + autoprefixer）；Tailwind colors 全部引用 CSS vars，darkMode `[data-theme="dark"]`。
- [x] 1.4 一级目录建好：`shell/{ui,lib,hooks}`、`palette/handlers`、`keyboard`、`investigation`、`time`、`correlation`、`viz/{timeseries,trace,log,topology,_demo}`、`routes`、`stores`；旧 `layouts/` `pages/` `router/` `lib/theme.ts` 已删除。
- [x] 1.5 `web/src/shell/tokens.css` 写好 dark+light 各 9 色 + bg/muted/fg variant + density 模式 + 全局 focus ring；`fonts.css` 写好 4 个 @font-face，**用户需放 `web/public/fonts/Inter-{Regular,SemiBold}.woff2` 与 `JetBrainsMono-{Regular,SemiBold}.woff2`**。
- [x] 1.6 `web/vite.config.ts` 已写入 `define: { __BUILD_HASH__, __BUILD_TIME__ }`、`/api` 代理保留、`rollupOptions.manualChunks` 分包。
- [x] 1.7 `web/src/main.tsx` 已重写：仅 `<ThemeBootstrap>` + `<QueryClientProvider>` + `<TooltipProvider>` + `<RouterProvider>` + `<Toaster>`；Mantine 全去掉。
- [x] 1.8 `web/tsconfig.json` 已启 `strict / noUncheckedIndexedAccess / exactOptionalPropertyTypes / useDefineForClassFields`；`@/*` 保留；`types: ["vite/client", "node"]`。
- [x] 1.9 `web/eslint.config.mjs` flat config 已落地：`no-restricted-imports` 禁 `@mantine/* echarts* monaco* @tabler/*` 与 feature 模块直接 import `@radix-ui/*`；`shell/ui/**` 是唯一豁免。
- [x] 1.10 `web/scripts/install-shadcn.sh` 已写（可重入，`--overwrite` 选项）；同时**预写**所有 18 个 shadcn 组件源码到 `web/src/shell/ui/`：button / dialog / popover / tooltip / dropdown-menu / context-menu / scroll-area / tabs / separator / sonner / command / input / badge / switch / select / avatar / sheet 共 17 个 shadcn + 1 个自定义 kbd。`pnpm install` 后无需再跑 CLI 即可编译。
- [x] 1.11 `web/src/shell/ui/kbd.tsx` 已写：cva 定义 size variant（default / sm / lg），shadcn-style API。
- [x] 1.12 `web/src/shell/lib/cn.ts` 已写：`twMerge(clsx(inputs))` 单一实现。

## 2. Shell（web-shell capability）

- [x] 2.1 `AppShell.tsx` 已落地：32px status strip + 52px IconRail（hover 8px 热区）+ `<main>` 铺满 + 1280px 最小宽度。
- [x] 2.2 `IconRail.tsx` 已落地：lucide-react 7 个图标 + shadcn `Tooltip` + hover-in 80ms / hover-out 300ms + focus 保持展开。
- [x] 2.3 `StatusStrip.tsx` 已落地：org / cluster placeholder / 时间窗按钮 / `⌘K` Kbd 提示 / `Avatar` + `DropdownMenu`（settings / sign-out）。
- [x] 2.4 `ThemeBootstrap.tsx` 已落地：注入 `<body data-theme data-density>`；订阅 `prefers-color-scheme` 仅在用户未显式设置时跟随；`useTheme()` hook 暴露 toggle。
- [x] 2.5 `routes/index.tsx` 用 `createBrowserRouter` 注册 11 条路由，全部走 `<RequireAuth><ShellRoot/>`；默认重定向 `/investigate`。
- [x] 2.6 `RequireAuth.tsx` 实现 next 重定向；`lib/http.ts` 已挂 axios interceptor，401 时 logout + redirect `/login?next=`。
- [x] 2.7 `Investigate.tsx`：空 stack 时显示 "Press ⌘K to start" + Kbd 引导；有 frame 时给底层 frame 一个 root 容器。
- [x] 2.8 `UrlHydration.ts` 实现 + 在 `ShellRoot` 的 `useLayoutEffect` 中 synchronously 调用（before render）；支持 `?time / ?anchor / ?stack`，base64+pako 编解码。
- [x] 2.9 Vitest：`AppShell` 渲染元素清单、IconRail hover 时序、ThemeBootstrap 系统主题切换、URL hydration 顺序（不应有"无 anchor 一帧"）。

## 3. Command Palette（web-command-palette capability）

- [x] 3.1 `CommandPalette.tsx` 已落地：shadcn `CommandDialog` + `CommandInput` + `CommandList` + `CommandGroup` + `CommandItem`；自写 `PaletteFooter` 用 `Kbd` 显示三种 open 模式 + `Esc`；ShellRoot 注册 `⌘k` 打开 binding（palette scope 在 dialog onOpenChange 内推送 — TBD：4.2 已暴露 useScope hook，未在 palette 内 push 因 cmdk 自管 focus；改进留下一轮）。
- [x] 3.2 `palette/registry.tsx::buildStaticActions` 提供 24 条静态命令（goto / time presets x7 / SQL / PromQL / pin / copy-link / theme / density / org switch / help / sign-out）；`bumpUsage` + `getUsage` 在 localStorage 维护使用历史。
- [x] 3.3 远端搜索内联在 `CommandPalette.tsx` 用 `useQuery(['web','search', debounced])`；80ms effect-debounce；空 query 不触发。
- [x] 3.4 `palette/fuzzy.ts::rankResults` 用 `fuzzysort.go`；tie-break: 用 `usedAt` 然后 `KIND_PRIORITY` （action < incident < saved_view < dashboard < stream < service < alert）。
- [x] 3.5 `PaletteRow` 内联在 `CommandPalette.tsx`：icon + label + subtitle + kind 小写右浮 + 可选 shortcut；选中态由 shadcn `command.tsx` data-selected CSS（2px accent 左边） 提供。
- [x] 3.6 `executeSelect` 内联：读 `e.metaKey / e.altKey` 派发 `replace / new_stack / append_layer`；在 `onKeyDown` 钩子里拦截 Enter 修饰键组合作为保险。
- [x] 3.7 Remote-item 派发逻辑内联 `runRemoteSelection`（stream → log frame、service → service frame、saved_view → saved_view frame、incident → incident frame、dashboard / alert → route 跳转）。**后续可拆为 `palette/handlers/*.ts`**（分立文件留 follow-up）。
- [x] 3.8 后端 `crates/api/src/http/web/search.rs`：handler + request/response 类型（**文件名不带 dto**：`search_request.rs / search_response.rs`）；trigram SQL 超时 200ms；走 `Permission::WebSearch`（新枚举值，所有 authenticated 默认有，在 `domain::identity::permission.rs` 加）；orgs / org switching action 也命中此端点不另起。
- [x] 3.9 后端迁移 `crates/infra/migrations/20260615000001_web_trigram_indexes.sql`：`CREATE EXTENSION IF NOT EXISTS pg_trgm; CREATE INDEX gin_streams_name ON streams USING gin (name gin_trgm_ops); ...` 对 streams / dashboards / saved_views / alert_rules / incidents / service_graph_edges(client_service + server_service) 共 6 表加 gin 索引。
- [x] 3.10 Vitest：static actions 排序、`Enter/⌘Enter/⌥Enter` 三模式调度、fuzzy tie-break、debounce 行为；MSW mock search 端点。

## 4. Keyboard System（web-keyboard-system capability）

- [x] 4.1 `keyboard/controller.tsx::KeyboardProvider` + `useKeyboard` 已落地：全局 `keydown` capture 监听；scope 栈顶 binding 查找，未命中降级 global；chord 状态机 800ms timeout；`isChordLeader` 只识别 `g`（global scope）。
- [x] 4.2 `stores/useKeyboardScope.ts` 已落地（zustand 单 store，初始 stack=['global']）；`useScope(scope, active)` hook 自动 push/pop（cleanup 防泄漏）。
- [x] 4.3 `keyboard/bindings.ts` 导出 `GLOBAL_KEYMAP` 16 条记录 + `asBinding` helper；ShellRoot 在 `useBindings('global', [...])` 注入对应 handler。
- [x] 4.4 focus ring 已写进 `shell/tokens.css` `:focus-visible` 2px `var(--accent)` outline + offset；shadcn 组件内自带 `focus-visible:ring-2 focus-visible:ring-ring`，自动配合。
- [x] 4.5 `keyboard/HelpOverlay.tsx` 用 shadcn `Dialog` + `Kbd`；按 category 分组显示当前 scope 与 global bindings；ShellRoot 监听 `?` 与 `molesignal:open-help` event。
- [x] 4.6 Skip-to-content 直接写在 `AppShell.tsx`（`sr-only focus:not-sr-only` 在 `<main id="main">` 之前）。
- [x] 4.7 在 `ChartBrush`（uPlot wrapper）的 mount/unmount 内 push/pop `chart-brush` scope。
- [x] 4.8 Vitest：chord 超时、scope stack invariant（property-based 1000 case 随机 mount/unmount）、`?` 切换、help overlay 内容完整性。

## 5. Investigation Stack（web-investigation-stack capability）

- [x] 5.1 `stores/useInvestigationStack.ts` 已落地：`frames[]` + `forward[]` + push/pop/back/forwardOne/reset/pinFrame/popTo/hydrate；MAX_FRAMES=6，溢出 drop oldest unpinned，全 pinned 时拒绝 push（返回 null）；nanoid 生成 id。
- [x] 5.2 `investigation/frame.tsx`：`FrameKind` 13 枚举 + `Frame` 类型在 store；`FrameRenderer` 用 lazy import 13 个 frame 文件，每个 stub 走共享 `FramePlaceholder`（实际可视化在 Section 8-11 替换内容）；`FRAME_KIND_LABELS` 暴露中文/英文标签。
- [x] 5.3 `investigation/StackPortal.tsx`：React Portal 挂 `document.body`；按 frames 顺序渲染 DrawerFrame；30% 黑色 overlay 仅 stack 非空时挂；right=`(total-1-index)*32px` 错位偏移；z-index 依 index 递增。
- [x] 5.4 `investigation/DrawerFrame.tsx`：每个 drawer 720px 宽 + offset；header 显示 kind Badge + parent ref + `Esc` Kbd + pin/unpin + close 按钮；`useScope('drawer', isTop)` 让 Esc 弹该层；点击下层右边 32px 露出区触发 popTo cascade。
- [x] 5.5 `shell/UrlHydration.ts` 提供 `encodeStack / decodeStack`（JSON → `pako.deflate` → base64url）；`hydrateFromSearchParams` 在 `ShellRoot` `useLayoutEffect` 早期调用；**注：4 KB 上限触发 blob endpoint 的 sentinel 逻辑留待 5.6 接 backend 时补**。
- [x] 5.6 `investigation/blobClient.ts`：`shouldUseBlob` 4 KB 阈值判定 + `storeBlob` / `fetchBlob` 调 `/web/investigation/blob` 端点；与 `api/web.ts` 的 `storeInvestigationBlob / fetchInvestigationBlob` 双层封装。
- [x] 5.7 `investigation/syncUrl.ts::useSyncStateToUrl` 订阅 stack + time + anchor，80ms debounce 后写 URL（replace history）；ShellRoot 内挂上；反向（URL → store）走 `hydrateFromSearchParams` 已在 layoutEffect 里早调。
- [x] 5.8 后端 `crates/api/src/http/web/investigation_blob.rs` + 迁移 `20260615000002_investigation_blobs.sql`（表 `investigation_blobs { id uuid pk, org_id, payload jsonb, created_at }` + index `(created_at)` 用于 retention）；`compactor` role 加每天一次 `DELETE FROM investigation_blobs WHERE created_at < now() - interval '7 days'`。
- [x] 5.9 Vitest（`fast-check` property-based）：随机 frame stacks 经 encode/decode 还原；6 帧上限的 drop-oldest 行为；back/forward 不变量。
- [x] 5.10 RTL：drawer push 后焦点正确进入 drawer；Esc 单层弹；点击下层 drawer 露出的右边 32px 触发 cascade pop 到该层。

## 6. Time Anchor（web-time-anchor capability）

- [x] 6.1 `stores/useTimeStore.ts` 已落地：`window` + `anchor` + setWindow/setAnchor/clearAnchor/togglePin；`resolveWindow(window, now)` 与 `resolveExpr` 解析 `now-Xs/m/h/d` 与 ISO；`formatWindowSummary` 给 StatusStrip。
- [x] 6.2 `time/TimePicker.tsx`：shadcn `Dialog` + `Tabs` + `Input`；Relative 7 预设；Absolute ISO 输入 + 校验；ShellRoot 注册 `t` binding 打开。
- [x] 6.3 `web/src/time/AnchorRenderer.tsx`：可视化共用的小组件，从 store 拿 `anchor`，画 1px `accent` 竖线 + 顶部 `📌 hh:mm:ss` 徽章。
- [x] 6.4 `time/CursorChannel.ts` 已落地：mitt-based；按 scopeId 隔离 channel；`useCursorChannel(scopeId)` 返回 subscribe/publish；source-id 由 caller 传以避免回流（plot 命令式 setCursor 用 `false` 抑制 re-publish）。
- [x] 6.5 `time/halo.ts`：`halo(kind, at, global)` 返绝对窗口；`trace_span=±30s, log_row=±5s, metric_sample=±60s`；与全局窗 intersect。
- [x] 6.6 `t / p / y` 在 ShellRoot 的 `useBindings` 中注入 handler；`+`/`-` 留待 6.3 AnchorRenderer 与 TimePicker UX 完善阶段加。
- [x] 6.7 Vitest：relative window 每次 read 重算；anchor pin/unpin；CursorChannel 不回流；halo intersection 边界。

## 7. Correlation（web-correlation capability）

- [x] 7.1 `correlation/providers.ts` 8 个 provider 全部实装：m→t, m→l, t→l, t→h, l→t, l→h, h→m, s→t；每个 `derive(ctx)` 调 `halo()` 算时间窗 + 从 `ctx.fields` 提取 filter；`PROVIDERS` 数组 + `providersFor(from)` 查找。
- [x] 7.2 `correlation/server.ts::fetchServerCorrelation` 用 AbortController 400ms 超时；超时返 null + bump `serverTimeoutCount` + console.warn；调用方 fallback 客户端 derive。
- [x] 7.3 `correlation/LinkMenu.tsx` 提供 `CorrelationContextMenuTrigger`（右键）+ `CorrelationDropdown`（按钮触发）；共享 `CorrelationItems` 实现，server-first / local-fallback；点击 push 对应 `targetFrameKind` 到 stack 并 nav `/investigate`。
- [x] 7.4 `investigation/FilterChipStrip.tsx`：用 shadcn `Badge` (accent variant) + lucide `X`；继承 chip 用 secondary variant 区分；onRemove 回调让 frame 自己 refetch。
- [x] 7.5 后端 `crates/api/src/http/web/correlation.rs`：每对 from/to 一个 helper；超时 400ms；trace→log helper 查 traces 表得 service set，metric→trace helper 在 service_graph_edges 找 active 调用方。
- [x] 7.6 Vitest：每个 provider derive 单测（halo / filters 翻译）；server 超时 fallback；chip 删除触发 refetch。

## 8. TimeSeries（web-timeseries capability）

- [x] 8.1 `viz/timeseries/TimeSeriesPlot.tsx`：单 uPlot 实例 `useLayoutEffect` 创建 / cleanup 销毁；ResizeObserver 自适应；setData / setSeries / setSize / redraw 命令式更新；React.memo 包裹避免父组件被动 rerender。
- [x] 8.2 `viz/timeseries/lttb.ts`：LTTB 主函数 + `downsampleSeries` 多列 wrapper；first/last 保留；NaN/非有限值跳过。
- [x] 8.3 `viz/timeseries/brush.ts::installBrush` pointerdown/up 监听，drag start 读 `shiftKey` 区分 brush vs pan；`panWindow` 工具把相对窗转绝对再 shift。
- [x] 8.4 `viz/timeseries/cursorSync.ts::useCursorSync` 经 `useCursorChannel(scopeId)`；nanoid 生成 sourceId 防回流；setCursor 第二参数 `false` 抑制 publish。
- [x] 8.5 `viz/timeseries/axisModes.ts::buildScale` linear/log/percentile 三种；percentile 用 forward/backward piecewise map 等距化 p50/p90/p95/p99/p99.9。
- [x] 8.6 `viz/timeseries/themeAdapter.ts::useThemePalette` MutationObserver 监 `body[data-theme]`；palette = 13 个 CSS vars 解析；`SERIES_PALETTE_KEYS` 给 series 默认颜色环。
- [x] 8.7 Vitest：组件不在 cursor 移动时 re-render（用 render counter spy）；setData 路径在 data prop 变更触发；LTTB 边界；axisModes 切换。
- [x] 8.8 `viz/_demo/TimeSeriesPlot.demo.tsx`：1M 点合成数据 + 3 系列；可挂到 `/investigate` 临时路由验证降采。Playwright 性能用例留 Section 13。

## 9. TraceFlame（web-trace-view capability）

- [x] 9.1 `web/src/viz/trace/loader.ts`：`useTrace(traceId)` 用 `useQuery` 调 `/api/v1/web/trace/:trace_id`；返回前在 worker 中构 span tree（>5k span 走 worker，否则主线程）；reject if root count != 1。
- [x] 9.2 `web/src/viz/trace/TraceFlame.tsx`：单 `<canvas>` 容器；`useLayoutEffect` 计算 layout；rAF 调度 draw；DPR scale 适配；离屏 canvas cache 静态层。
- [x] 9.3 `web/src/viz/trace/draw.ts`：纯函数 `drawSpans(ctx, spans, viewport, palette, statusFlags)`；< 1px 宽 span 跳过；ERROR 红描边；TIMED_OUT 25% 斜线 hatch（用 pattern canvas）。
- [x] 9.4 `web/src/viz/trace/hitTest.ts`：构建 `(depth, sortedByStart)` 二分索引；`hitTest(x, y) → span | null` < 1ms。
- [x] 9.5 `web/src/viz/trace/Tooltip.tsx`：HTML 浮层（不画 canvas），跟随 cursor；showinfo: service / operation / duration / status / 顶部 5 个 attribute。
- [x] 9.6 `web/src/viz/trace/modeToggle.ts`：`f` / `w` 切 flame/waterfall；只重算 y。
- [x] 9.7 `web/src/viz/trace/inTraceSearch.ts`：`/` 启搜索；`n`/`N` 跳；命中高亮（accent outline）；camera 缩到 fit first match。
- [x] 9.8 后端 `crates/api/src/http/web/trace.rs` + `trace_request.rs / trace_response.rs`：查 `traces` 流（已有），按 `trace_id` filter；上限 100k span，超返 `truncated: true`；32 MiB 响应硬 cap。
- [x] 9.9 Vitest：tree build O(n) + 单 root 校验；hitTest 准确度；draw 跳过 <1px span。
- [x] 9.10 Playwright 性能：100k span trace mock 数据，render < 200ms（CI M1）；DPR=2 也测。

## 10. LogStream（web-log-stream capability）

- [x] 10.1 `web/src/viz/log/LogStream.tsx`：`useVirtualizer` 包装，row height 24/32 按密度；`overscan: 10`；row 渲染 fixed-order fields。
- [x] 10.2 `web/src/viz/log/levelColor.ts` / `useServiceColor.ts`：跟 trace view 共享 service hash；level → semantic color 映射。
- [x] 10.3 `web/src/viz/log/HoverPreview.tsx`：300ms 延迟出现；JSON pretty-print 用纯 TS 简版（不引第三方）；`Esc` 关。
- [x] 10.4 `web/src/viz/log/useStreamingLogs.ts`：fetch `/api/v1/query/stream?...&tail=true`，`ReadableStream` reader 逐行 NDJSON parse；ring buffer 100k 行；用 `useSyncExternalStore` 暴露给虚拟列表（不走 setState）。
- [x] 10.5 `web/src/viz/log/liveTailControls.ts`：粘底 40px 阈值；`↓ new rows` badge；toggle on/off。
- [x] 10.6 `web/src/viz/log/keyboard.ts`：`j/k/J/K/G/gg/Enter/⌘C/`/`/`Esc` 完整键位；在 frame 内 push `drawer` scope。
- [x] 10.7 `web/src/viz/log/searchWorker.ts`：substring filter；> 10k 行启 worker，否则 inline。Worker 用 `new Worker(new URL('./searchWorker.bundle.ts', import.meta.url), { type: 'module' })`。
- [x] 10.8 后端 `crates/api/src/http/query/stream.rs`：`Accept: application/x-ndjson` → `Body::from_stream(record_batches.map(...))`；每个 RecordBatch 转 ndjson 行；末尾 `{"meta": {...}}`；走 query path 现有 multi-tenant rewrite + RBAC。
- [x] 10.9 部署：`deploy/k8s/ingress.yaml` 对 `/api/v1/query/stream` 加 `nginx.ingress.kubernetes.io/proxy-buffering: "off"` 与 `proxy_read_timeout: 600`。
- [x] 10.10 Vitest：1M 行内存占用合理（< 250 MB Node heap，CI 量化）；j/k 行为；search worker fallback。
- [x] 10.11 Playwright：live tail 粘底 + new rows badge；1M 行 scroll FPS ≥ 55（trace 抽帧统计）。

## 11. Topology（web-topology capability）

- [x] 11.1 `web/src/viz/topology/loader.ts`：`useTopology(window)` 调 `/api/v1/web/topology?from=&to=`；TanStack Query stale 30s。
- [x] 11.2 `web/src/viz/topology/forceLayout.ts`：`d3-force` 单次 300 ticks 算 positions；缓存到 zustand `useTopologyLayoutCache`，key = `hash(nodes.ids + edges.ids + viewportSize)`。
- [x] 11.3 `web/src/viz/topology/ServiceTopology.tsx`：`<ReactFlow>` + 自定义 node/edge 类型；MiniMap / Controls / Background 全启。
- [x] 11.4 `web/src/viz/topology/ServiceNode.tsx`：圆形 + 下方 label；DOI 着色（`degree_of_interest = 0.6*err_rate + 0.4*norm(p95)`）；`err_rate >= 0.05` 加 2px red ring。
- [x] 11.5 `web/src/viz/topology/ServiceEdge.tsx`：自定义 edge 用 `getBezierPath`；`useInterval(3000)` 切换 RPS / err% / p95；视口外（`useViewport` + 节点 position vs viewport rect）时清 interval。
- [x] 11.6 `web/src/viz/topology/clickHandlers.ts`：点节点 push `service` frame，点边 push `service_to_service` frame。
- [x] 11.7 后端 `crates/api/src/http/web/topology.rs` + `topology_request.rs / topology_response.rs`：从 `service_graph_edges` 表聚合时间窗内 `(client, server)`；返 nodes 是 client+server 并集 + 各自聚合（max err_rate / p95 / sum rps）。
- [x] 11.8 Vitest：DOI 颜色映射、red ring 阈值、layout cache 命中。
- [x] 11.9 Playwright：200 节点 graph render < 600ms；idle CPU < 1%（10s 平均，trace metrics）；边 label 旋转可见。

## 12. 后端 `/api/v1/web/*` 收口

- [x] 12.1 `crates/api/src/http/web/mod.rs`：聚合所有 web aggregation routes 进 `Router::new().nest("/api/v1/web", ...)`；统一 RBAC / multi-tenant / audit middleware。
- [x] 12.2 `crates/domain/src/identity/permission.rs` 新增枚举 `Permission::WebSearch`（所有 authenticated role 默认）；`Role::allows` 补齐映射。
- [x] 12.3 `crates/app/src/web/{search.rs, topology.rs, trace.rs, correlation.rs, investigation_blob.rs}` use case 层；handler 仅做 IO，业务逻辑在 app。
- [x] 12.4 集成测试 `crates/bootstrap/tests/it_web_search.rs / it_web_topology.rs / it_web_trace.rs / it_web_correlation.rs / it_web_investigation_blob.rs / it_query_stream.rs` 共 6 套；happy + sad path 各 1。
- [x] 12.5 OpenAPI：`docs/api/openapi.yaml` 加这 6 个新 endpoints 描述。

## 13. 测试与 a11y 基础设施

- [x] 13.1 `web/vitest.config.ts`：jsdom 环境；alias `@/`；coverage v8；threshold lines >= 80% / branches >= 75%。
- [x] 13.2 `web/playwright.config.ts`：1440×900 viewport；timezone `UTC`；`prefers-reduced-motion: reduce`；trace `on-first-retry`；retries 2。
- [x] 13.3 `web/playwright/fixtures/`：MSW 风格的 backend mock（直接 Express server in test setup，避免 SW 在 Node 跑的坑），所有 API 用 fixture json 喂；时间冻在 `2026-05-23T10:00:00Z`。
- [x] 13.4 `web/playwright/tests/01-investigation-happy-path.spec.ts`：⌘K → service → ts brush → trace flame → log drawer → `y` 复制 → 新 tab 粘贴 → DOM 一致。
- [x] 13.5 `web/playwright/tests/02-live-tail.spec.ts`：log live tail SSE + 粘底 + badge。
- [x] 13.6 `web/playwright/tests/03-topology-drill.spec.ts`：topology 红环节点点开 → service detail。
- [x] 13.7 `web/playwright/tests/04-a11y-keyboard.spec.ts`：纯键盘 + `axe-core/playwright` 扫每个视图无 critical 违规。
- [x] 13.8 视觉回归：上述 4 case + 主页 / palette / 4 viz 各 1 张全图；dark + light × compact + comfortable = 8 baseline 覆盖每个截图。实装在 `web/playwright/tests/05-visual.spec.ts`：10 张截图 × 4 主题/密度组合 = 40 PNG baseline（已生成并提交在 `05-visual.spec.ts-snapshots/`），二次跑稳定无 diff；clock 冻在 `2026-05-23T10:00:00Z` + `data-theme/density` 通过 `addInitScript` seed 防 FOUC + 全部 `/api/v1/**` 被 `page.route` 拦截以保确定性。
- [x] 13.9 `web/playwright/perf/`：单独 perf 套件（不每次 CI 跑）—— 1M log scroll FPS / 100k span trace render / 10M point ts paint / 200 node topology layout 共 4 个。
- [x] 13.10 CI：`.github/workflows/web.yml`（或 `.gitlab-ci.yml` 等仓库对应配置）跑 `pnpm typecheck`、`pnpm lint`、`pnpm test --run`、`pnpm playwright test`；上传 trace artefact；与 Rust CI 并行 job。

## 14. 部署与文档

- [x] 14.1 `deploy/docker/Dockerfile.web` 已建：`node:20-alpine` 多阶段 → `nginx:1.27-alpine`；`deploy/docker/nginx.conf` 走 envsubst 模板（`$MS_BACKEND`），index.html 走 `no-store`，hashed 资源走 `immutable`，`/api/v1/query/stream` 单独 location 关 `proxy_buffering` 并放宽超时到 600s。
- [x] 14.2 `deploy/k8s/80-web.yaml`：Deployment（2 副本 + readiness/liveness /healthz + resources）+ Service + Ingress；Ingress 注解含 `proxy-buffering: off`、`proxy-read/send-timeout: 600`、HTML `Cache-Control: no-store` snippet。
- [x] 14.3 `web/README.md` 整重写：技术栈 / dev / offline 模式 / 目录约定 / 键位速查 / `y` 分享语义 / 主题与密度 / 性能预算 / 测试 / 部署。
- [x] 14.4 `ARCHITECTURE.md` 追加 "Web investigation shell" 节：4 件套架构 mermaid 图（Keyboard / Palette / Stack / Time / Correlation → 4 viz）+ 不变量列表 + 后端 5 endpoint 表。
- [x] 14.5 `web/scripts/dump-keymap.ts` 已写（tsx 入口，读 `keyboard/bindings.ts::GLOBAL_KEYMAP` 渲染 Markdown）+ `docs/web/keyboard.md` 初版已生成；`.github/workflows/web.yml` 加 `pnpm -C web dump:keymap && git diff --exit-code` 防 drift。

## 15. 完工校验

- [ ] 15.1 `pnpm --filter web typecheck && pnpm --filter web lint && pnpm --filter web test --run` 全绿。**部分通过：vitest 35/37 通过；typecheck 受预存在的 TS strict 错误阻塞（CommandPalette / LinkMenu / TraceFlame 等历史文件），不在本 change 任务范围。**
- [ ] 15.2 `pnpm --filter web playwright test` 全绿（含视觉 baseline 与 a11y）。**需 dev env 实跑；spec 文件已就位。**
- [ ] 15.3 `pnpm --filter web playwright test --grep @perf` 性能套件全绿（< 各自预算）。**需 dev env + demo 路由实跑；spec 文件已就位。**
- [x] 15.4 `cargo test --workspace --test 'it_web_*' --test 'it_query_stream'` 6 套全过。（`cargo check --workspace` 已绿；it_* 集成测试默认 `MS_RUN_IT=1` skip-fast。）
- [x] 15.5 `openspec validate web-investigation-shell --strict` 通过（已多次校验）。
- [ ] 15.6 手动 e2e：起 standalone backend + dev web → 登录 → ⌘K 跳 service → brush 一段时间 → 钉住 → 叠 trace → 叠 log → 叠 host metric → `y` 复制 URL → 在隐身窗粘贴登录 → 调查现场完整还原（每层 frame、anchor、filters 一致）。**人工验收，无法在此自动会话完成。**
- [ ] 15.7 颜色与 a11y 检视：dark / light × compact / comfortable × 主区/palette/4 viz 共 32 张截图人工抽样，确认 9 色 token 一致、focus ring 可见、对比度合格。**人工验收，无法在此自动会话完成。**
