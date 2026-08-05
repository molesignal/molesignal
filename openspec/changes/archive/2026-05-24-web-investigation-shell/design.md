## Context

现 `web/` 是 Mantine + React Router + ECharts + Monaco 的常规管理控制台：
- `web/src/layouts/AppShellLayout.tsx` 是带固定 nav 的 `<AppShell>`；`router/index.tsx` 全部映射到 `pages/<resource>/<X>Page.tsx`，每个页面独立加载、跨页跳转走 react-router；`stores/auth.ts` 是唯一 zustand 全局态。
- 可视化全部 ECharts（`echarts-for-react`）、查询编辑器是 Monaco 全量包；图标 Tabler；UI 库 Mantine v7 + `@mantine/dates / form / code-highlight / notifications`。
- 后端 `production-core-engine` change 已经把 query / ingest / trace / alert / dashboard / saved view / pipeline / function / quotas / audit / service_graph / OTLP / Prom 等全量 API 拉通，HTTP 路径 200+；其中 traces 的 service_graph 聚合 `/api/v1/traces/service_graph` 和 streaming SQL（`Accept: application/x-ndjson`）已经落地，因此本前端 change 不需要后端再造同源能力，仅追加四条 web 聚合便捷路径。

产品定位是 dev/SRE 在 incident 调查中追根因——和 Linear / Vercel / Datadog incident timeline 那一档"键盘驱动 + 上下文不丢"是同一档。Mantine 的"管理控制台"皮肤和 ECharts 的"渲染稳定但 API 很 React-unfriendly"两件事必须替掉。

约束：
- 后端 HTTP 契约不可大改（只能加 `/api/v1/web/*` 与 `/api/v1/query/stream`），保持与 backend 同步进度。
- 不引入 SSR / Next，纯 SPA + Vite，CDN 缓存策略不变。
- 仅支持桌面端，最小宽度 1280px；不做移动端 / 触屏。
- 受 backend `web-shell` 路由列出的 RBAC 限制；前端不实装额外权限模型。
- Tailwind 仅用于 chrome 一致性 class（间距 / 边距 / flex / grid），所有可视化和数据组件用 vanilla CSS / CSS-in-JS（避免 Tailwind 与 canvas / uPlot 主题切换之间的耦合）。

## Goals / Non-Goals

**Goals:**
- ⌘K 一等公民：键盘到达全功能；任何静态命令 + 任何用户实体（streams/dashboards/saved_views/alerts/incidents/services）可在 < 200ms 召回。
- 调查栈：右侧最多 6 层叠抽屉，URL 完整序列化，`y` 复制能在另一台机器 1:1 还原现场。
- 跨信号关联：8 条 metric ↔ trace ↔ log ↔ host ↔ service 预置链接 + 服务端 `correlation` endpoint 补充。
- 时间锚：全局 + 钉点 + 跨视图 cursor 同步；进入任意层调查抽屉默认继承，可临时 override。
- 四大可视化达标：uPlot 千万点 / canvas 火焰图 100k span / 虚拟滚动百万行 / React Flow 200 节点 60fps。
- 整个 shell 在中等笔记本（M1）从冷启到首屏 < 1.2s，且 ⌘K 首键到 palette 可输入 < 60ms。
- Playwright 端到端覆盖核心调查路径，视觉回归与键盘 a11y 必跑。

**Non-Goals:**
- 移动端 / 触屏 / 平板布局。
- 替代 Grafana panel 编辑器（dashboard 仍 import + 渲染 + 重查询，不在 web 端改 panel）。
- 富主题（仅 dark/light，不开放自定义主题）。
- AI / 自然语言查询输入（独立 change）。
- 内嵌 RUM session player（仅 metadata + 跳 trace）。
- 拓扑双向流量与 service mesh 五层视图（仅 RPC 调用有向图）。
- 离线 / PWA / Service Worker 模式。

## Decisions

### 1. UI 框架：shadcn/ui（owned source under `web/src/shell/ui/`）

**选择**：chrome 用 **shadcn/ui**——CLI `pnpm dlx shadcn@latest init` 把组件源码直接复制进 `web/src/shell/ui/`，依赖底层是 `tailwindcss` + `@radix-ui/react-*` primitives + `class-variance-authority` + `clsx` + `tailwind-merge` + `lucide-react`；shadcn 本身不是 npm 包，是"复制粘贴 + 你拥有源码"模式。

**为什么不继续用 Mantine**：Mantine 是 "管理控制台" 美学（圆角、阴影、稍大间距），与"安静的画布"反着；其 `AppShell` 强制 sidebar + header 概念，硬切语义代价大。

**为什么选 shadcn 而非裸 Radix**：用户明确指定 shadcn。实际收益是 (a) 现成的 Dialog / Popover / Tooltip / DropdownMenu / ContextMenu / ScrollArea / Tabs / Separator / Toaster (sonner) / Command 组件源码（自动 wire 好 Radix 行为 + Tailwind 样式）；(b) 升级走 `shadcn add --overwrite`，diff 可控，比包升级稳；(c) `cva` 已经在 shadcn 内嵌，我们的 variants（density / theme）直接复用；(d) 跟其他 SRE 调查工具的"键盘感"美学（Linear/Vercel/cmdk 官方 demo）同源。

**`components.json` 配置**：`style: "new-york"`、`baseColor: "neutral"`、`cssVariables: true`、`tailwind.css: "src/shell/tokens.css"`、`tailwind.config: "tailwind.config.ts"`、`aliases: { components: "@/shell/ui", utils: "@/shell/lib/cn", hooks: "@/shell/hooks" }`。所有 shadcn 默认的 `--background` / `--foreground` / `--primary` / `--accent` / `--destructive` 等 token 在 `tokens.css` 内直接映射到我们 9 色 CSS vars，**不允许 shadcn 默认调色板渗到任何组件**——CI lint 加规则禁止在 `shell/ui/` 外直接使用 `bg-background`/`text-foreground` 这种 shadcn-internal class（强制走我们 9 色语义）。

**需要安装的 shadcn 组件清单**（一次性写在 `web/scripts/install-shadcn.sh`）：`button`、`dialog`、`popover`、`tooltip`、`dropdown-menu`、`context-menu`、`scroll-area`、`tabs`、`separator`、`sonner`、`command`（cmdk wrap）、`input`、`badge`、`switch`、`select`、`avatar`、`kbd`（自己加 variant）、`sheet`（用于 settings 二级抽屉，非投资栈）。投资栈的 drawer 不用 shadcn `sheet`（因为要 6 层叠且偏移定制），自写。

**约束**：shadcn 组件只能放在 `web/src/shell/ui/`；feature 模块（palette / keyboard / investigation / time / viz）只能从这里 import；任何 ad-hoc UI 元素都要先看 shadcn 是否够用，再决定是否新增。

**替代方案**：裸 Radix + 自写样式 —— 否决（用户指定 shadcn，且 shadcn 在 Radix 之上提供了大量已经调好的 dark/light 适配和 cva variants）。

### 2. ⌘K 内核：`cmdk` + 自研 fuzzy + 远端 search

**选择**：`cmdk` 提供命令列表、键盘焦点、a11y 行为；自研 `useCommandRegistry` 合并静态 actions 与远端 `GET /api/v1/web/search` 结果；用 `fuzzysort` 排序。

**为什么不用 Algolia / Meilisearch**：远端搜索本身就要打后端，再加外部服务边际收益 < 部署成本；后端已经把所有用户实体在 PG，写一个 `UNION ALL` SQL 就够。

**结构**：
```
<CommandPalette>          // wraps cmdk.Dialog
  <cmdk.Input />          // controlled input -> debounced query
  <cmdk.List>
    {staticActions.map(a => <cmdk.Item key={`action:${a.id}`}>...</cmdk.Item>)}
    {remoteResults.map(r => <cmdk.Item key={`${r.kind}:${r.id}`}>...</cmdk.Item>)}
  </cmdk.List>
  <PaletteFooter />       // 显示 Enter / ⌘Enter / ⌥Enter 三种 open 模式
</CommandPalette>
```

**远端 search 后端实现**：`api/src/http/web/search.rs` 单个 handler，执行：
```sql
(SELECT 'stream' AS kind, id, name AS label, stream_type AS subtitle, ts_rank(...) AS rk FROM streams WHERE org_id=$1 AND name % $2)
UNION ALL ... (dashboards/saved_views/alerts/incidents/services)
ORDER BY rk DESC LIMIT $3
```
用 PG trigram (`pg_trgm`) `%` 算 ranking；index 已经在 backend migration 里加（本 change 不引入新 SQL extension，仅启用）。

**`⌘Enter` / `⌥Enter` 语义**：在 cmdk 的 `onSelect` 回调里读 `event.metaKey + altKey` 状态分支。

### 3. 全键盘系统：scope stack + 注册表

**选择**：键盘事件统一进 `KeyboardController`（一个 capture-phase `keydown` 监听），按当前 scope 栈顶查找 binding 表，找不到 fallback 到 global。

**为什么不用 react-hotkeys / mousetrap**：它们都是"扁平 keymap"，没有 scope 概念；我们的 `Esc` 必须按栈逐层弹（palette → drawer → brush → main），需要显式 scope 栈。

**数据结构**：
```ts
type Scope = 'global' | 'palette' | 'drawer' | 'chart-brush' | 'editor' | 'help-overlay';
type Binding = { keys: string[]; handler: (e) => void; description: string; };
const scopeStack: Scope[] = ['global'];  // top is last
const bindings: Map<Scope, Map<string, Binding>> = ...;
```

**chord (两键序列)**：维持 `pendingChord: string | null` + 800ms timer；`g s` 是 `pendingChord = 'g'` 然后 `'s'` 时触发。

**焦点环**：CSS `:focus-visible` + 2px `outline-offset: 2px outline: 2px solid var(--accent)`；列表用 `aria-activedescendant` 而不是逐项实际焦点（性能）。

**替代方案**：Mantine `Spotlight` —— 否决，Mantine 全包要剔除。

### 4. 调查栈：zustand store + URL ↔ state 双向同步

**选择**：`useInvestigationStack` 一个 zustand store；订阅 `react-router`'s `useSearchParams` 双向同步。

**Frame 类型枚举**：
```ts
type FrameKind = 'trace' | 'log' | 'metric' | 'host' | 'service'
              | 'incident' | 'sql' | 'promql' | 'dashboard_panel' | 'saved_view';
type Frame = {
  id: string;                // 客户端 nanoid
  kind: FrameKind;
  params: Record<string, unknown>;
  time_range_override?: TimeRange;
  anchor_override?: ISODateTime;
  parent_frame_id?: string;
  pinned: boolean;
  created_at: number;
};
```

**URL 编码**：`stack` 参数取 frames 的极简形态（剥掉 `created_at`, `id` 重新生成）→ JSON → `pako` deflate → `base64url`。`pako` 大小 ~30KB gzip，比直接 base64 节省 60%。

**长度阈值**：若 base64 后 > 4 KB，把 `params` 单独 `POST /api/v1/web/investigation/blob` 拿 `blob_id`，URL 里只放 `blob_id`。该 endpoint 是 ephemeral KV（PG 表 `investigation_blobs { id, org_id, payload_json, created_at }` + 7 天 TTL 由 compactor 清理），独立于核心数据流。

**可视化叠加**：CSS `position: fixed; right: 0; top: 32px; bottom: 0` 加 `transform: translateX(-(N - i) * 32px)` 把每个 drawer 错位 32px；最顶 drawer `z-index` 最高，`box-shadow: -2px 0 16px rgba(0,0,0,.4)` 形成深度感。

**为什么 6 层硬上限**：3 层是 SRE 调查最常见深度；6 层留出双倍空间给"再叠一层验证"；超过 6 层屏幕宽度（1280px - 32px top - 6×32 offset - 720 width）就开始撑爆，且认知负担也太重。

**替代方案**：tab 组（horizontal tabs）—— 否决，tabs 是平行结构，不传达"父子调查关系"；按 timeline 排（vertical accordion）—— 否决，每层都要展开看，反复操作慢。

### 5. 时间锚：global store + frame override + CursorChannel

**选择**：单一 `useTimeStore`：
```ts
type TimeState = {
  window: { from: TimeExpr; to: TimeExpr; mode: 'relative'|'absolute' };
  anchor: { at: ISODateTime; label?: string } | null;
};
```
每个 frame 可携 `time_range_override` / `anchor_override`，可视化在 mount 时从 React Context 拿到自己的 effective window。

**CursorChannel**：每个 frame 一个 `mitt`-style EventEmitter（独立，互不串台），全局主区另一份；图表 mount 时订阅 + 卸载时退订。`mitt` 100 行 dep-free，比手卷 `Subject` 省事。

**为什么 cursor 不进 zustand**：cursor 每帧都变，会高频触发 zustand subscribers 全更新；pub/sub 直接走 emitter 单独通道。

### 6. Correlation：客户端 provider + 服务端补强

**客户端 provider 表**：
```ts
const providers: LinkProvider[] = [
  { from: 'metric',  to: 'trace', label: 'View traces',  derive: m2t },
  { from: 'metric',  to: 'log',   label: 'View logs',    derive: m2l },
  { from: 'trace',   to: 'log',   label: 'View logs',    derive: t2l },
  { from: 'trace',   to: 'host',  label: 'View host',    derive: t2h },
  { from: 'log',     to: 'trace', label: 'View trace',   derive: l2t },
  { from: 'log',     to: 'host',  label: 'View host',    derive: l2h },
  { from: 'host',    to: 'metric',label: 'View metrics', derive: h2m },
  { from: 'service', to: 'trace', label: 'View traces',  derive: s2t },
];
```
每个 `derive` 是纯函数 `(ctx) => CorrelationContext`，halo 时间窗 / filter 翻译规则全在客户端可静态分析。

**服务端 `/api/v1/web/correlation`**：当 `from_kind = 'trace'` 时，可以扫 trace 内所有 span 的 service set 当作 filter；当 `from_kind = 'metric'` 时，可以查最近匹配的 trace_id 候选。客户端先在 400ms 内尝试拿到，超时 fallback 客户端 `derive`。

**过滤继承**：UI 上 chip 化（`<FilterChipStrip filters={inheritedFilters + addedFilters}>`），每个 chip 支持 `×` 删除 → frame 自动 refetch。

### 7. TimeSeriesPlot 自封装 uPlot

**结构**：
```tsx
function TimeSeriesPlot({ data, window, axes, onRangeSelect, onCursorMove, theme, height }) {
  const ref = useRef<HTMLDivElement>(null);
  const plotRef = useRef<uPlot | null>(null);

  useLayoutEffect(() => {
    const opts: uPlot.Options = buildOpts({ window, axes, theme, height, onRangeSelect, onCursorMove });
    plotRef.current = new uPlot(opts, prepareData(data), ref.current!);
    return () => plotRef.current?.destroy();
  }, []); // 只创建一次

  useEffect(() => {
    plotRef.current?.setData(prepareData(data));
  }, [data]);

  useEffect(() => {
    if (theme) plotRef.current?.setSize({ width: ref.current!.clientWidth, height });
    // 主题切换走 plot.redraw() 用新的 stroke / fill
  }, [theme, height]);

  return <div ref={ref} className="ts-plot" style={{ height }} />;
}
```

**Shift+drag brush**：uPlot 自带 `select` 模块，监听 `setSelect` hook；判断 `e.shiftKey` 决定 emit `onRangeSelect` 或 pan。

**Cursor sync**：订阅 `CursorChannel`，把外部 t 转 x，调 `plot.setCursor({ left: x }, false)`（`false` 表示不触发同 channel emit）。

**LTTB downsample**：纯 JS 实现 ~30 行；目标点数 = `chartWidth * downsampleHint`；只对 `data.length > target * 2` 才跑。

**Theme tokens**：传 `--accent` 等 CSS var 进 uPlot stroke 配置；theme 切换通过 `useEffect` 改 stroke + grid color。

**为什么不用 echarts**：echarts API 是 option-driven、React 包装层 (`echarts-for-react`) 每次 prop 变 reconcile 慢；uPlot 直接 canvas、API 命令式但配合 ref + setData 很顺；千万点性能 uPlot 是工业标杆。

**替代方案**：`@visx/visactual`—— 否决，visx 是 SVG-based，万点级开始掉帧；`Apache Echarts GL`—— 否决，超过我们 scope。

### 8. TraceFlame：canvas + d3-scale

**选择**：
- 数据：构建 `SpanNode { span, depth, x, y, w }` 树，O(n) DFS 计算 x/w（按 `start_ns - root_start` / `total_duration * pixelWidth`），y = `depth * rowHeight`。
- 渲染：单 `<canvas>`，`requestAnimationFrame` 触发的 `draw()` 只在 viewport 或 data 变化时调用。
- 染色：`hashService(span.service) % 9`-color 调色板；`status=ERROR` 额外 1px red 描边。
- 交互：mouse `clientX` → time via `d3.scaleLinear`，二分查 span（按 `(depth, x range)` 桶分），hover 时切换的是 React state（只触发 tooltip div re-render，不重画 canvas）。
- 100k span 性能：culling（rect 完全在 viewport 外的不画）+ 1px 以下的 span 不画 + offscreen canvas DPR cache。
- `f`/`w` 切换：mode = 'flame' 用 y = depth；mode = 'waterfall' 用 y = orderIndex(start_ns)；只重算 y 不重算 x。

**为什么不引入 `react-flame-graph`**：那库不支持 100k 量级 + 不支持 status outline + 不支持 ctx 同步；最小可用代码 < 400 LOC，自写够清晰。

**离屏 canvas**：对静态层（spans）画一次到 `OffscreenCanvas`，主 canvas 每帧 `drawImage(offscreen, ...)` + 叠 cursor/tooltip。

### 9. LogStream：TanStack Virtual + custom row

**选择**：`useVirtualizer({ count, estimateSize: () => 24, overscan: 10 })`；自定义 row component fully controlled 由 virtualizer position。Live tail 走 `EventSource` 或 `ReadableStream` (fetch chunked) 读 NDJSON，每帧 append 后调 `virtualizer.measure()`。

**字段染色**：tokens 用 CSS vars；level / service 映射跟 trace view 共享 `useServiceColor(service)` hook 保证全局一致。

**Hover preview**：`onMouseEnter` 上的 row 设 `hoveredRowId`，浮窗 React 节点根据 virtualizer item rect 计算 absolute top。

**`/` 搜索 filter**：客户端做（rows 已经在内存）；用 `Web Worker`（`comlink` 或原生）跑 substring scan 避免阻塞主线程（>10k 行时启用 worker）。

**为什么不用 react-window**：TanStack Virtual 支持动态 size + Web Worker 友好 + 与 TanStack Query 同源；react-window 旧且不维护。

### 10. ServiceTopology：React Flow + d3-force

**选择**：
- 节点：`reactflow.Node` `type: 'service'`，自定义 component 画 circle + label。
- 边：`reactflow.Edge` `type: 'service-edge'`，custom edge 用 `getBezierPath` 生成路径 + 自维护 rotating label。
- Layout：mount 时一次性 `d3-force` 跑 300 ticks，写回 `nodes[i].position`；缓存在 zustand `useTopologyLayoutCache.set(graphHash, positions)`。
- DOI 着色：`degree_of_interest = 0.6*error_rate + 0.4*normalize(p95_ms, max=1000ms)`；映射到 5-stop `green→yellow→red` 颜色（线性插值 HSL）。
- 边 label 轮播：在 `<ServiceEdge>` 内 `useInterval(3000)` 切换；React Flow 自带 viewport 状态可以读 `useViewport()`，当节点完全在视口外时清 interval。

**为什么不用 Cytoscape**：Cytoscape 是非 React 库，包很大（>200KB gzip），React 绑定也常年滞后；React Flow 200KB gzip 但 React 原生，扩展能力强。

**替代方案**：纯 d3 SVG—— 否决，200 节点拖动时 SVG re-render 慢；ReactFlow + 自定义节点是工业级折中。

### 11. 数据层：TanStack Query + zustand 分工

**约定**：
- 远端数据 → `useQuery` / `useInfiniteQuery`；所有 query key 形如 `['/api/v1/...', orgId, params]`。
- UI 局部态 (palette open, drawer state) → `useState` / `useReducer`。
- 跨组件全局 UI 态 (time, stack, scope) → zustand。
- Mutation 后 `queryClient.invalidateQueries({ queryKey: ['/api/v1/...'] })` 收口。

**SSE / NDJSON 流式查询**：自己写 `useStreamingQuery({ url, language, statement, tail })` hook，内部维持 ring buffer（最近 100k 行），rows 用 `useSyncExternalStore` 暴露给 LogStream 避免每条都 setState。

### 12. 主题与 token

**唯一来源**：`web/src/shell/tokens.css` 定义 `:root[data-theme='dark']` / `:root[data-theme='light']` 两套 CSS vars；Tailwind config 把这些 var 映射进 `theme.extend.colors`。uPlot / Canvas 渲染时通过 `getComputedStyle(root).getPropertyValue('--accent')` 读。

**等宽字体**：`JetBrains Mono` 通过 woff2 self-host（不接 Google Fonts，避免外网依赖）；Inter 同。`web/public/fonts/` 各放 4 个 weight。

**主题切换**：写 `<body data-theme="...">`；可视化通过 `useTheme()` hook（订阅 `MutationObserver` on `data-theme`）触发 redraw。

### 13. 后端 `/api/v1/web/*` 实现位置

每个 endpoint 放 `crates/api/src/http/web/<endpoint>.rs`：
- `search.rs`: 单 PG 查询 + trigram；返回 `WebSearchResponse { items: [...] }`；权限 = `Permission::WebSearch`（新枚举值，所有登录用户默认有）；时间预算 200ms（query timeout）。
- `topology.rs`: 查 `service_graph_edges WHERE org_id=$1 AND ts BETWEEN $2 AND $3`，聚合到 `(client, server) → avg(rps), max(err_rate), p95(duration)`；返回 nodes 是 client/server 并集。
- `trace.rs`: 直接 `SELECT * FROM traces WHERE org_id=$1 AND trace_id=$2 ORDER BY start_ns`，组装 span tree；上限 100k span / 32 MiB 响应；超阈截断 + 返 `truncated: true`。
- `correlation.rs`: switch on `(from, to)`，调相应 helper（trace 的 fetch service set / metric 的 fetch trace_id candidates 等）；超时 400ms。
- `query/stream.rs`: 把 `QueryService::run` 的 `RecordBatch` 流式输出，每个 batch 转 NDJSON 行；不缓存。Response 用 `axum::response::Sse` 或 `Body::from_stream`。

每个 handler 走与 backend `production-core-engine` 同套 `validate → service → IntoResponse`、走 `quotas / audit / RBAC` 中间件、走 `multi-tenant planner rewrite`。

### 14. 测试矩阵

- **Vitest**：单元 + 集成（jsdom 模拟）。覆盖：
  - keyboard scope 栈 push/pop 与 binding 路由（≥ 95% lines）。
  - investigation stack URL 双向序列化（property-based with `fast-check`，1000 case）。
  - time anchor relative resolution，halo 时间窗算子，CursorChannel 节流。
  - command registry merge + fuzzy ranking + tie break。
  - LTTB downsample 算法（对 100k 输入校验 first/last 点保留）。
  - service color hash 稳定性（同 service 同 session 同色）。
- **React Testing Library**：组件 visual 行为（palette open/close/select、drawer push/pop、chip remove → refetch）。
- **Playwright**：4 条端到端 path：
  1. **Investigation happy path**：login → ⌘K → 跳 service api → 看 timeseries → shift-drag 选峰 → 叠开 trace flame → 点 span → 叠开 log → 复制 URL → 新 tab 粘贴 → 还原一致。
  2. **Live tail**：开 log live tail → 后端 mock SSE → 验证滚动粘底 + new rows badge。
  3. **Topology drill**：服务图 → 点红环节点 → 叠开 service detail → 切到 trace。
  4. **A11y keyboard tour**：纯键盘走完上面 path 1，无鼠标；axe-core 扫所有视图无 critical violation。
- **视觉回归**：每个 spec page 跑 `screenshot({ fullPage: true })` baseline；diff > 0.5% 失败；4 个主题 × 2 密度 = 8 张基线。
- **性能预算**（在 CI 用 `playwright tracing` + lighthouse-like 自检）：
  - cold first paint < 1200ms（M1 笔记本）。
  - ⌘K 首键 → palette 可输入 < 60ms。
  - 1M-row log scroll FPS ≥ 55。
  - 100k-span trace render < 200ms。
  - 200-node topology force layout + first paint < 600ms。
  - 10M-point timeseries first paint < 80ms（已 downsample）。
  - 任何 frame push（drawer 打开）< 100ms。

## Risks / Trade-offs

- **[uPlot 的 React 包装容易抖动]** → 严格只在 mount 创建一次实例，所有更新走 `setData / setSize`；任何 prop 变化经 ref 转命令式 API；新增 unit test 覆盖 "setState 不触发 uPlot 重建"。
- **[Canvas 火焰图与 React 状态同步复杂]** → tooltip 用独立 React DOM（不画在 canvas）、selection state 走 React state；canvas 只负责 spans 本身 + 可选 highlight stroke；与 React 边界清晰。
- **[投资 URL 编码膨胀]** → base64+deflate 通常 < 1.5 KB；超 4 KB 走 `investigation_blob` endpoint；blob 表 7 天 TTL；分享场景对短期可用足够。
- **[键盘 scope 栈泄漏导致 Esc 失灵]** → 每个 push/pop 都在 React `useEffect` cleanup 里强制 pop；CI 加 "scope stack invariant" test：任何 fiber 卸载后 stack 必须不含其 pushed scope。
- **[shadcn / Tailwind 与 canvas / uPlot 主题切换耦合]** → 所有色彩用 CSS vars 单一来源；shadcn `components.json` 强制 `cssVariables: true`；`tokens.css` 把 shadcn 默认 token（`--background` 等）重映射到我们 9 色；Tailwind config 把 vars 引出来；uPlot / canvas 通过 `getComputedStyle` 读 var；主题切换 `<body data-theme>` 触发 MutationObserver → 可视化 redraw。
- **[React Flow 自定义 edge 性能]** → 视口 culling 仅做 timer 暂停，不动 React Flow 自身节点；CPU idle 测试在 CI 跑 10s。
- **[TanStack Virtual 在 row size 变化时跳]** → live tail 启用 `measureElement` ref；先以 estimate 渲染 → 测真实 size → 二次定位；用户层面表现是首条新 row 偶尔 1 帧偏移，可接受。
- **[Web Worker 在 jsdom 环境难测]** → 单元测试用 main-thread fallback path（小行数）；Worker 仅在 prod >10k 行时启用；Playwright 跑真 Worker 验证。
- **[后端 search 走 ANY UNION 太慢]** → 提前在迁移里给 `streams(name) / dashboards(title) / saved_views(name) / alerts(name) / incidents(fingerprint) / services(name)` 加 `gin_trgm_ops` 索引；查询 timeout 200ms。
- **[Streaming SQL chunked body 与 proxy 缓冲]** → 部署文档明确 nginx / ingress 上对 `/api/v1/query/stream` 关闭 `proxy_buffering`；浏览器侧 fetch stream + ReadableStream reader 拉取，浏览器自带不缓冲。
- **[趋同色板与可访问性冲突]** → 9 色都对暗/亮主题双背景做 contrast >= 4.5:1 验证（CI 跑 `axe-core/playwright`）；degree-of-interest 5-stop ramp 额外做色弱（deuteranopia）模拟 baseline 校验。
- **[fonts 自托管增加首屏]** → `<link rel="preload">` woff2 + `font-display: swap`；首屏只用 1 weight 400 + 600，其它按 lazy。
- **[Playwright 视觉 baseline 不稳]** → 固定 viewport 1440×900；冻 `Date.now()` mock；屏蔽动效（reduce-motion）。
- **[投资栈 6 层硬上限触达]** → 提供"投资栈历史"侧抽屉（按 `gh` 触发）展示完整链路，含已被丢弃的 frame timeline；不是栈，只是 timeline。

## Migration Plan

1. **依赖与 scaffold**：新建 `web/src/{shell,palette,keyboard,investigation,time,correlation,viz,routes,stores}` 8 目录与 `web/src/shell/tokens.css`；`web/package.json` 跑 `pnpm add` + `pnpm remove` 一次性切依赖。
2. **保留 API 客户端**：`web/src/api/*` 保留不动；新增 `web/src/api/web.ts`（消费 `/api/v1/web/*`）、`web/src/api/stream.ts`（streaming query）。
3. **后端 endpoint 上线**：先把 `crates/api/src/http/web/{search,topology,trace,correlation,investigation_blob}.rs` 与 `crates/api/src/http/query/stream.rs` 实装；migrations 加 trigram 索引 + `investigation_blobs` 表。前端在 mock 起步阶段先用 MSW 桩。
4. **第一阶段（chrome + 时间锚 + ⌘K + 投资栈）**：跑通骨架 + palette + 主区放一张静态 placeholder，可以键盘到达各 route，调查栈能 push/pop/URL 序列化；这一阶段 acceptance：Playwright "happy path" 跑除可视化外的所有键盘步骤通过。
5. **第二阶段（四大可视化）**：依次落 TimeSeriesPlot → LogStream → TraceFlame → ServiceTopology；每个独立可发布。
6. **第三阶段（correlation 与 frame loader）**：实装 8 条 link provider + 服务端 correlation；把 frame 与对应可视化绑定，跑完整 Playwright path。
7. **回滚**：本 change 是 `web/` 整树替换；如发布后回滚，docker image tag 切回上一版即可（后端独立）。后端新增的 5 个 endpoint 是纯增量，回滚 web 不影响后端。

## Open Questions

- **是否需要 `gj` / `gk` 大跳（PageDown/PageUp 风格）**？倾向"是"，在 log stream 里有 1M 行 j/k 太慢；本期实装 `J = 10 rows` `K = -10 rows` `G = bottom` `gg = top`（vim 风），明确写进 keyboard help。
- **`Esc` 在 main view 是什么行为**？倾向"清空当前搜索框 / 退出 brush 选区"，没有 brush / 搜索时则不动；本期默认这样，help overlay 注明。
- **palette 是否需要支持反向命令历史（`↑` like shell）**？本期不做，等用户反馈；预留 `usedAt` 字段为以后排序。
- **trace flame 是否支持 mini-map 缩略图**？本期不做，靠键盘 `n/N` 跳匹配；mini-map 留后续。
- **拓扑图是否支持边按方向过滤（仅入边 / 仅出边）**？倾向"是"，但本期靠点击节点叠 `service` frame 解决（默认显示该服务的入边出边各 top 5）；图本身不做边方向 filter。
- **投资栈是否支持"分支"（同一父 frame 跳两次产生两支）**？本期为线性栈，不支持分支；多于一次跳就替换上层（保留 forward buffer）。
