## Why

现有 `web/` 是 Mantine + ECharts + Monaco 的"管理控制台"形态：左侧固定菜单、每页一张大图、所有跨视图跳转必须回菜单重选，时间窗、`trace_id`、`service` 这些 SRE 调查时手里的"线索"在跨页之间全部丢失。这恰好与产品定位反着——molesignal 的核心受众是 incident 中追根因的 SRE / dev，他们不需要"一眼看懂的仪表盘"，需要的是 **⌘K 一键进入、键盘从 metric → trace → log → host 无限下钻、每一步上下文都不丢** 的调查工具。本 change 把整个 `web/` 推倒重做成"安静的画布 + 深邃的调查流"，对齐 Linear / Vercel 那一档键盘体验，并补齐火焰图 / 时序图 / 虚拟滚动日志 / 拓扑图 四大支撑可视化。

## What Changes

- **BREAKING**：删除 `web/src/layouts/AppShellLayout.tsx`、`web/src/router/index.tsx` 现有结构、`web/src/pages/` 下既有页面骨架；保留 `web/src/api/*` HTTP 客户端与 `web/src/types/*` 协议类型作为复用基础。Mantine + Tabler + ECharts + echarts-for-react 全部下线，替换为下方依赖矩阵。
- **极简 shell**：新 `AppShell` 只有一条 32px 顶部 status strip（org / 在线集群 / 当前时间窗 / ⌘K 提示 / 当前用户）+ 一条默认折叠、hover 8px 边触发的 52px 图标导航。**没有面包屑、没有页头、没有页脚**，主区铺满。
- **⌘K 命令面板（一等公民）**：基于 `cmdk` 自研 `CommandPalette`。能"点到的都能键盘到"——跳服务 / 切时间窗 / 运行 SQL / PromQL / 跳告警 / 跳 incident / 改 dashboard / 收藏 saved view / 切 org / 切主题 / 打开 ⌘K 内嵌 docs。命令源：静态 action registry + 动态远端搜索（streams / dashboards / saved views / alerts / incidents / services）。`⌘K` 打开、`Esc` 关、`⌘Enter` 在新调查栈打开、`⌥Enter` 在当前栈追加一层。
- **全键盘下钻**：新增 `KeyboardSystem`（全局 keymap + scope 栈 + focus ring + `?` 帮助叠层）。固定键位：`j/k` 移动、`Enter` 进入、`Esc` 退一层抽屉、`⌘[ / ⌘]` 调查栈前进后退、`g s / g a / g d / g t / g l` 跳 Services / Alerts / Dashboards / Traces / Logs、`t` 打开时间窗 picker、`p` 钉住当前时间、`/` 在当前视图聚焦搜索、`y` 复制可分享调查 URL。
- **调查栈（investigation stack）**：新增 `InvestigationStack` —— 主区右侧最多 6 层可叠加抽屉，每层是一个 `InvestigationFrame { kind, params, time_range_override?, parent_frame_id }`。Trace span → 抽屉里看 span 详情 → 点 host 字段叠层看主机指标 → 点尖峰叠层看相关日志，每一层独立有 `j/k/Enter/Esc`，`Esc` 退一层、`⌘[` 后退、`⌘]` 前进，整条栈编码进 URL `?stack=<base64-json>` 用于 `y` 一键分享。每层右上角显示 `^esc` 提示。
- **全局时间轴 + 钉住锚点**：`TimeAnchor` 顶部组件——下拉 / 输入相对（`-15m / -1h / -24h`）或绝对窗口、上下方向键步进、`p` 把当前焦点选区"钉住"为锚（在 UI 上画一条竖线 + 显示 `📌 09:42:31 UTC`）；所有时序图 / 火焰图 / 日志列表都围绕锚对齐渲染。钉子在调查栈每一层共享，但每层可临时 override。
- **跨信号关联（correlation）**：新增 `CorrelationRouter` —— 任意视图里点一个含上下文的字段（`trace_id` / `service.name` / `host` / `severity` / `_timestamp`）都触发一次"叠层跳"，时间窗 + 标签条件自动注入到下一层。三大信号之间的链接表内置：metric→trace（按 service + time bucket）、trace→log（按 trace_id + 时间窗）、log→host（按 host + 时间窗）、host→metric、log→trace 等共 8 条。
- **四大可视化组件**：
  - `TimeSeriesPlot`（uPlot 0.11 自封装 React，**不引入 echarts**）：支持鼠标框选 → emit `range_selected({from, to})` 给上层钻取、cursor 同步（多图共用 cursor）、`log/linear/percentile` 轴切换、最多 24 系列、暗 / 亮主题 token、千万点级 downsample。
  - `TraceFlame`（基于 `d3-scale` + `<canvas>` 自研，**不引入 d3 完整包**）：火焰图 / 瀑布图双视图切换；hover span 显示 inline tooltip、点击 span 推一层调查栈；按 service 染色 + 错误 span 红描边；支持 100k span 量级（canvas 离屏 + DPR 适配）；内置 `/` 搜索高亮命中 span。
  - `LogStream`（TanStack Virtual + 自研 row 渲染）：百万行不卡的虚拟滚动；字段着色（level / service / trace_id）；hover 行右浮 mini preview；live tail（SSE / chunked）；`j/k` 移动、`Enter` 展开 raw JSON、`⌘C` 复制选中行；`/` 列内搜索。
  - `ServiceTopology`（React Flow + 自定义 node/edge renderer）：节点 = 服务，边 = RPC 调用；边标签轮播 `RPS / err% / p95`；节点按"degree of interest"（与当前 anchor 时间窗内异常程度）着色；点节点叠层进 service 详情；支持 200+ 节点 force layout + viewport culling。
- **可分享 URL 状态**：所有调查状态进 URL —— route（`/investigate`）、`?time=<iso>..<iso>`、`?anchor=<iso>`、`?frame=<kind:params>`、`?stack=<base64-json>`、`?filters=<base64-json>`。粘贴链接 = 复现调查现场，包含调查栈每一层与 anchor。
- **主题与字号**：单 dark / light 主题；色板限定为 **背景 / 表面 / 主色 / 强调（橙）/ 红 / 绿 / 黄 / 蓝 / 紫** 9 色，禁止额外色；等宽字体（JetBrains Mono）做数据、Inter 做 chrome；密度模式 `comfortable / compact`，compact 是 SRE 默认。shadcn 的 `components.json` 锁 `style: "new-york"`、`baseColor: "neutral"`、`cssVariables: true`，所有 shadcn 组件的 `--background` / `--foreground` / `--primary` 等 token 映射到我们 9 色 CSS vars（统一在 `tokens.css`），不允许 shadcn 默认 token 渗到组件里。
- **数据层重写**：`@tanstack/react-query` 升 v5（已有）+ `zustand` v5（已有），新增：
  - `useTimeAnchor` zustand store：global window + pinned anchor + cross-frame override 解析。
  - `useInvestigationStack` zustand store：frame stack push/pop/back/forward + URL 双向序列化。
  - `useKeyboardScope` zustand store：当前生效的 scope 栈（global / palette / drawer / chart-brush）。
  - `useCommandRegistry`：静态注册 + 远端 search 合并，`fuzzysort` 做模糊匹配。
- **后端契约新增（仅 web 侧消费的轻量接口）**：
  - `GET /api/v1/web/search?q=...&types=streams,dashboards,saved_views,alerts,incidents,services&limit=20`：⌘K 远端搜索聚合。
  - `GET /api/v1/web/topology?from=&to=`：返 `{ nodes: [{id, name, error_rate, p95}], edges: [{source, target, rps, err_rate, p95}] }`。
  - `GET /api/v1/web/trace/:trace_id`：返完整 span 树（含 attribute）。
  - `GET /api/v1/web/correlation/:from_kind/:to_kind?ctx=<base64>`：返目标信号的预填查询（`{ sql?, promql?, time_range, filters }`）。
  - `GET /api/v1/query/stream?...`（SSE / chunked NDJSON）：log live tail 与 streaming search 路径。
- **测试**：Vitest + React Testing Library 全量替换；Playwright 端到端跑 ⌘K → 时间窗切换 → 钉住 → metric→trace→log 三层叠层 → `y` 复制链接 → 新页面粘贴还原 整条 happy path；视觉回归用 `@playwright/test` screenshot baseline；键盘 a11y 用 `axe-core/playwright`。
- **非目标**：移动端 / 小屏布局（最小宽度 1280px）、Grafana panel 编辑器替代（仍 import-only）、可视化主题深度可配（仅 dark/light）、第三方 SSO 登录页改造（仍走 backend `auth/sso/login` 跳转）、富文本 dashboard 编辑（dashboard model 仅展示与重新查询，不在 web 端编辑 panel JSON）、AI / 自然语言查询输入（独立 change）、内嵌 RUM session player（仅展示 session 元数据 + 跳转到 trace）、Service mesh 拓扑的双向流量（仅显示有向边 RPC 调用）。

## Capabilities

### New Capabilities

- `web-shell`：极简 chrome（顶部 status strip + 折叠侧栏 + 主区铺满）、主题 token、路由地图、auth 引导、density 模式。
- `web-command-palette`：⌘K 命令面板（cmdk 内核 + action registry + 远端搜索 + 模糊匹配 + ⌘Enter/⌥Enter 双开模式）。
- `web-keyboard-system`：全局键位（j/k/Enter/Esc/⌘[/⌘]/g 系列/t/p/y/?）+ scope 栈 + focus ring + 帮助叠层 + a11y。
- `web-investigation-stack`：右侧叠层调查抽屉模型（push/pop/back/forward + URL 序列化 + 跨层 anchor 共享）。
- `web-time-anchor`：全局时间窗 + 可钉住锚点 + 跨视图 cursor 同步 + 调查栈每层 override。
- `web-correlation`：metric ↔ trace ↔ log ↔ host 双向跨信号链接表 + 上下文自动注入。
- `web-timeseries`：uPlot 自封装 React 组件（brush 钻取、cursor 同步、轴模式、千万点 downsample）。
- `web-trace-view`：canvas + d3-scale 火焰图 / 瀑布图（100k span、染色、搜索高亮、点击叠层）。
- `web-log-stream`：TanStack Virtual 虚拟滚动日志（百万行、字段染色、live tail、`j/k/Enter`）。
- `web-topology`：React Flow 服务拓扑图（节点 degree-of-interest 着色、边轮播 RPS/err/p95、200+ 节点）。

### Modified Capabilities

无既有前端 capability。后端契约这一侧新增了 `/api/v1/web/*` 与 `/api/v1/query/stream` 两组路由——它们的定义在本 change 的 `web-shell` capability 内描述（前端消费契约），不修改 backend 的 `query` / `ingestion` 等 spec。后端实现这些路由的 task 列在 `tasks.md` 中。

## Impact

- **代码**：`web/` 整树替换（保留 `web/src/api/`、`web/src/types/`）；新建 `web/src/{shell,palette,keyboard,investigation,time,correlation,viz,routes,stores}` 八个一级目录；新增 `playwright/` 端到端测试目录。
- **依赖（新增）**：UI 框架走 **shadcn/ui**（`pnpm dlx shadcn@latest init` 自动复制组件到 `web/src/shell/ui/`，自管样式与升级）—— shadcn 引入 `tailwindcss ^3.4`、`tailwindcss-animate`、`class-variance-authority ^0.7`、`clsx ^2.1`、`tailwind-merge ^2.5`、`lucide-react ^0.460`、按需 `@radix-ui/react-*` primitives（dialog / popover / tooltip / dropdown-menu / scroll-area / toast 即 sonner / tabs / separator / context-menu / slot），全部 shadcn CLI 自动写入 `package.json`；可视化用 `cmdk ^1.0`、`uplot ^1.6`、`@tanstack/react-virtual ^3.10`、`reactflow ^11.11`、`fuzzysort ^3.0`、`d3-scale ^4.0`、`d3-array ^3.2`、`d3-force ^3`、`zustand ^5.0`、`@tanstack/react-query ^5.59`、`react-router-dom ^6.27`、`dayjs ^1.11`、`pako ^2.1`、`mitt ^3.0`、`nanoid ^5`、`@codemirror/state`、`@codemirror/view`、`@codemirror/lang-sql`、`@codemirror/lang-promql`；测试 `vitest ^2.1`、`@testing-library/react ^16`、`@playwright/test ^1.48`、`axe-core ^4.10`、`fast-check ^3.22`。
- **依赖（移除）**：`@mantine/*` 所有 7 个包、`echarts`、`echarts-for-react`、`monaco-editor`、`@monaco-editor/react`（query 编辑器替换为更轻的 `@codemirror/state + @codemirror/view + @codemirror/lang-sql` + `@prometheus-io/codemirror-promql`——CodeMirror 6 官方无 PromQL 模式，Prometheus 自家发的 `@prometheus-io/codemirror-promql` 是唯一可用 PromQL mode；本 change 在落 SQL/PromQL 编辑器 frame 时再 `pnpm add` 该包）、`@tabler/icons-react`（改用 `lucide-react ^0.460`）。
- **后端 API（新增）**：`GET /api/v1/web/search`、`GET /api/v1/web/topology`、`GET /api/v1/web/trace/:trace_id`、`GET /api/v1/web/correlation/:from/:to`、`GET /api/v1/query/stream`（SSE）。
- **CI**：新增 `web` workflow——`pnpm i --frozen-lockfile && pnpm typecheck && pnpm lint && pnpm test --run && pnpm playwright test`；artefact 上传 playwright trace。
- **部署**：`web/` 构建产物路径不变；`deploy/k8s` 增 ingress `Cache-Control: no-store` 头给 HTML（hash 资源走默认强缓存）；不影响后端容器。
