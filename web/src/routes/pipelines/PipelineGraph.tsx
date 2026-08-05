import { useQuery } from '@tanstack/react-query';
import {
  Braces,
  CheckCircle2,
  CircleAlert,
  Database,
  Filter,
  LayoutGrid,
  PanelRightClose,
  PanelRightOpen,
  Plus,
  Send,
  Trash2,
  X,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import {
  addEdge,
  Background,
  BackgroundVariant,
  Controls,
  Handle,
  Position,
  ReactFlow,
  useEdgesState,
  useNodesState,
  type Connection,
  type Edge,
  type Node,
  type NodeProps,
  type ReactFlowInstance,
} from 'reactflow';
import 'reactflow/dist/style.css';

import * as connectorsApi from '@/api/connectors';
import * as functionsApi from '@/api/functions';
import type { PipelineInput, ScheduledPipeline } from '@/api/pipelines';
import * as streamsApi from '@/api/streams';
import { ChromeButton, Pill, type PillTone } from '@/shell/chrome';
import { CodeEditor } from '@/shell/codeEditor';
import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';

export type PipelineSignalType = 'logs' | 'metrics' | 'traces';

/** 单个 VRL 处理步骤（按顺序串行应用）。 */
export interface TransformStep {
  name: string;
  script: string;
}

export interface PipelineGraphModel {
  signalType: PipelineSignalType;
  sources: string[];
  sinks: string[];
  transforms: TransformStep[];
  retryPolicy: string;
}

type GraphNodeKind = 'source' | 'transform' | 'sink';

interface GraphNodeData {
  kind: GraphNodeKind;
  label: string;
  subtitleKey: string;
  tone: PillTone;
}

const DEFAULT_VRL_SCRIPT = `. = parse_json!(.message)

.environment = "production"
.cluster = "us-east-1"
.level = downcase(.level || "info")

if exists(.trace_id) {
    .trace.id = .trace_id
    del(.trace_id)
}`;

// 内置 VRL 转换预设目录已迁到后端（vrl_presets 表 + GET /vrl_presets），前端按需拉取。

type GraphIssueLevel = 'error' | 'warning';

export interface GraphIssue {
  level: GraphIssueLevel;
  /** i18n code（`graph.validation.<code>`），UI 侧渲染。 */
  code: string;
  params?: Record<string, string | number>;
}

/** 从 React Flow 节点 id 解析种类（source-0 / transform-1 / sink-0）。 */
function nodeKindFromId(id: string | null | undefined): GraphNodeKind | null {
  if (!id) return null;
  const match = /^(source|transform|sink)-(\d+)$/.exec(id);
  return match ? (match[1] as GraphNodeKind) : null;
}

/**
 * 连线校验：只允许「前向」连接 - source → transform/sink、transform → transform/sink。
 * 禁止连入 source、连出 sink、自环；transform→transform 仅允许指向更靠后的步骤（避免成环）。
 */
export function isValidGraphConnection(conn: Connection): boolean {
  const from = nodeKindFromId(conn.source);
  const to = nodeKindFromId(conn.target);
  if (!from || !to || conn.source === conn.target) return false;
  if (to === 'source' || from === 'sink') return false;
  if (from === 'transform' && to === 'transform') {
    const fromIndex = Number(conn.source!.split('-')[1]);
    const toIndex = Number(conn.target!.split('-')[1]);
    return toIndex > fromIndex;
  }
  return true;
}

/** 流水线图校验：返回错误/警告列表（`code` 在 UI 侧用 i18n 渲染）。 */
export function validateGraph(
  model: PipelineGraphModel,
  connectorIds: string[] = [],
): GraphIssue[] {
  const issues: GraphIssue[] = [];
  const sources = model.sources.map((item) => item.trim()).filter(Boolean);
  const sinks = model.sinks.map((item) => item.trim()).filter(Boolean);
  if (sources.length === 0) issues.push({ level: 'error', code: 'no_source' });
  if (sinks.length === 0) issues.push({ level: 'error', code: 'no_sink' });
  if (model.transforms.length === 0) issues.push({ level: 'error', code: 'no_transform' });

  const dupSource = firstDuplicate(sources);
  if (dupSource) issues.push({ level: 'warning', code: 'duplicate_source', params: { name: dupSource } });
  const streamSinks = sinks.filter((sink) => !sink.startsWith('connector:'));
  const dupSink = firstDuplicate(streamSinks);
  if (dupSink) issues.push({ level: 'warning', code: 'duplicate_sink', params: { name: dupSink } });
  for (const sink of streamSinks) {
    if (sources.includes(sink)) {
      issues.push({ level: 'warning', code: 'source_is_sink', params: { name: sink } });
    }
  }

  model.transforms.forEach((step, index) => {
    if (!step.name.trim()) {
      issues.push({ level: 'warning', code: 'transform_name_missing', params: { index: index + 1 } });
    }
    if (!step.script.trim()) {
      issues.push({
        level: 'error',
        code: 'transform_script_missing',
        params: { name: step.name.trim() || String(index + 1) },
      });
    }
  });

  if (connectorIds.length > 0) {
    const known = new Set(connectorIds);
    for (const sink of sinks) {
      if (sink.startsWith('connector:')) {
        const id = sink.slice('connector:'.length);
        if (!known.has(id)) issues.push({ level: 'warning', code: 'connector_missing', params: { id } });
      }
    }
  }
  return issues;
}

export function pipelineGraphStats(model: PipelineGraphModel): {
  nodes: number;
  edges: number;
} {
  const nodes = model.sources.length + model.transforms.length + model.sinks.length;
  const edges = model.transforms.length === 0
    ? 0
    : model.sources.length + Math.max(0, model.transforms.length - 1) + model.sinks.length;
  return { nodes, edges };
}

function firstDuplicate(items: string[]): string | null {
  const seen = new Set<string>();
  for (const item of items) {
    if (seen.has(item)) return item;
    seen.add(item);
  }
  return null;
}

const SIGNAL_TONE: Record<PipelineSignalType, PillTone> = {
  logs: 'orange',
  metrics: 'blue',
  traces: 'green',
};

const DEFAULT_GRAPH: Record<
  PipelineSignalType,
  { sources: string[]; sinks: string[]; transformName: string }
> = {
  logs: {
    sources: ['app_logs'],
    sinks: ['app_logs_enriched'],
    transformName: 'normalize-logs',
  },
  metrics: {
    sources: ['app_metrics'],
    sinks: ['metrics_rollup'],
    transformName: 'rollup-metrics',
  },
  traces: {
    sources: ['app_traces'],
    sinks: ['traces_normalized'],
    transformName: 'normalize-traces',
  },
};

const NODE_TYPES = { pipeline: PipelineGraphNode };

export function signalTypeFromPipeline(
  pipeline: ScheduledPipeline | null | undefined,
  fallback: PipelineSignalType = 'logs',
): PipelineSignalType {
  const steps = stepObject(pipeline?.function_steps);
  const candidates = [
    steps.signal_type,
    pipeline?.description,
    pipeline?.source_stream,
    pipeline?.target_stream,
    pipeline?.name,
    JSON.stringify(pipeline?.function_steps ?? ''),
    fallback,
  ]
    .map((value) => String(value ?? '').toLowerCase())
    .join(' ');
  if (candidates.includes('metric')) return 'metrics';
  if (candidates.includes('trace')) return 'traces';
  return 'logs';
}

export function pipelineGraphFromPipeline(
  pipeline: ScheduledPipeline | null | undefined,
  fallbackType: PipelineSignalType = 'logs',
): PipelineGraphModel {
  const signalType = signalTypeFromPipeline(pipeline, fallbackType);
  const defaults = DEFAULT_GRAPH[signalType];
  const steps = stepObject(pipeline?.function_steps);
  const stepSources = stringsFrom(steps.sources);
  const stepSinks = stringsFrom(steps.sinks);
  const connectorSinks = stringsFrom(steps.sink_connectors).map((id) => `connector:${id}`);
  return {
    signalType,
    sources: unique(stepSources.length > 0 ? stepSources : stringsFrom(pipeline?.source_stream, defaults.sources)),
    sinks: unique([
      ...(stepSinks.length > 0 ? stepSinks : stringsFrom(pipeline?.target_stream, defaults.sinks)),
      ...connectorSinks,
    ]),
    transforms: transformsFromSteps(pipeline?.function_steps, defaults.transformName),
    retryPolicy: typeof steps.retry_policy === 'string' ? steps.retry_policy : 'exponential',
  };
}

export function defaultPipelineGraph(signalType: PipelineSignalType = 'logs'): PipelineGraphModel {
  const defaults = DEFAULT_GRAPH[signalType];
  return {
    signalType,
    sources: [...defaults.sources],
    sinks: [...defaults.sinks],
    transforms: [{ name: defaults.transformName, script: DEFAULT_VRL_SCRIPT }],
    retryPolicy: 'exponential',
  };
}

export function pipelineInputFromGraph({
  name,
  graph,
  cron,
  lookbackSecs,
  enabled,
}: {
  name: string;
  graph: PipelineGraphModel;
  cron: string;
  lookbackSecs?: number;
  enabled?: boolean;
}): PipelineInput {
  const defaults = DEFAULT_GRAPH[graph.signalType];
  const sources = normalizedList(graph.sources, defaults.sources);
  const allSinks = normalizedList(graph.sinks, defaults.sinks);
  const streamSinks = allSinks.filter((sink) => !sink.startsWith('connector:'));
  const connectorSinks = allSinks
    .filter((sink) => sink.startsWith('connector:'))
    .map((sink) => sink.slice('connector:'.length));
  const sourceStream = sources[0] ?? defaults.sources[0] ?? `${graph.signalType}_source`;
  const targetStream = streamSinks[0] ?? defaults.sinks[0] ?? `${graph.signalType}_target`;
  const transforms = graph.transforms.length > 0
    ? graph.transforms
    : [{ name: defaults.transformName, script: DEFAULT_VRL_SCRIPT }];
  return {
    name,
    source_stream: sourceStream,
    target_stream: targetStream,
    function_steps: {
      language: 'vrl',
      signal_type: graph.signalType,
      sources,
      sinks: streamSinks,
      sink_connectors: connectorSinks,
      retry_policy: graph.retryPolicy || 'exponential',
      steps: transforms.map((step) => ({
        transform_name: step.name.trim() ? step.name.trim() : defaults.transformName,
        script: step.script.trim() ? step.script : DEFAULT_VRL_SCRIPT,
      })),
    },
    cron,
    ...(lookbackSecs !== undefined && { lookback_secs: lookbackSecs }),
    ...(enabled !== undefined && { enabled }),
  };
}

export function PipelineGraphView({
  model,
  className,
}: {
  model: PipelineGraphModel;
  className?: string;
}) {
  return (
    <div className={cn('h-[280px] overflow-hidden rounded-md border border-bd-0 bg-bg-0', className)}>
      <PipelineGraphCanvas model={model} />
    </div>
  );
}

export function PipelineGraphEditor({
  value,
  onChange,
  className,
  defaultInspectorOpen = true,
  validationRequest = 0,
  readOnly = false,
  readOnlyReason,
}: {
  value: PipelineGraphModel;
  onChange: (next: PipelineGraphModel) => void;
  className?: string;
  defaultInspectorOpen?: boolean;
  validationRequest?: number;
  readOnly?: boolean;
  readOnlyReason?: string | undefined;
}) {
  const { t } = useTranslation('pipelines');
  const [selectedId, setSelectedId] = React.useState('transform-0');
  const [inspectorOpen, setInspectorOpen] = React.useState(defaultInspectorOpen);
  const [dock, setDock] = React.useState<'closed' | 'validation' | 'code'>('closed');
  const [layoutRequest, setLayoutRequest] = React.useState(0);
  const nodeIds = React.useMemo(
    () => [
      ...value.sources.map((_, index) => `source-${index}`),
      ...value.transforms.map((_, index) => `transform-${index}`),
      ...value.sinks.map((_, index) => `sink-${index}`),
    ],
    [value.sources, value.transforms, value.sinks],
  );

  React.useEffect(() => {
    if (!nodeIds.includes(selectedId)) setSelectedId('transform-0');
  }, [nodeIds, selectedId]);

  React.useEffect(() => {
    setInspectorOpen(defaultInspectorOpen);
  }, [defaultInspectorOpen]);

  React.useEffect(() => {
    if (validationRequest > 0) setDock('validation');
  }, [validationRequest]);

  const connectorsQuery = useQuery({ queryKey: ['connectors'], queryFn: () => connectorsApi.list() });
  const connectorIds = React.useMemo(
    () => (connectorsQuery.data ?? []).map((connector) => connector.id),
    [connectorsQuery.data],
  );
  const issues = React.useMemo(() => validateGraph(value, connectorIds), [value, connectorIds]);
  const errorCount = issues.filter((issue) => issue.level === 'error').length;
  const stats = React.useMemo(() => pipelineGraphStats(value), [value]);
  const selected = parseNodeId(selectedId);
  const selectedTransform = selected?.kind === 'transform'
    ? value.transforms[selected.index] ?? null
    : null;

  React.useEffect(() => {
    if (dock === 'code' && !selectedTransform) setDock('closed');
  }, [dock, selectedTransform]);

  const addSource = () => {
    if (readOnly) return;
    const next = `${value.signalType}_source_${value.sources.length + 1}`;
    onChange({ ...value, sources: [...value.sources, next] });
    setSelectedId(`source-${value.sources.length}`);
    setInspectorOpen(true);
  };
  const addTransform = () => {
    if (readOnly) return;
    onChange({ ...value, transforms: [...value.transforms, { name: '', script: DEFAULT_VRL_SCRIPT }] });
    setSelectedId(`transform-${value.transforms.length}`);
    setInspectorOpen(true);
  };
  const addSink = () => {
    if (readOnly) return;
    const next = `${value.signalType}_sink_${value.sinks.length + 1}`;
    onChange({ ...value, sinks: [...value.sinks, next] });
    setSelectedId(`sink-${value.sinks.length}`);
    setInspectorOpen(true);
  };

  const updateSelectedTransform = (patch: Partial<TransformStep>) => {
    if (readOnly || selected?.kind !== 'transform') return;
    onChange({
      ...value,
      transforms: value.transforms.map((transform, index) =>
        index === selected.index ? { ...transform, ...patch } : transform,
      ),
    });
  };

  return (
    <div
      aria-disabled={readOnly || undefined}
      className={cn(
        'flex flex-col overflow-hidden rounded-md border border-bd-0 bg-bg-1',
        readOnly && 'bg-bg-2',
        className,
      )}
    >
      <div className="flex min-h-11 flex-wrap items-center gap-2 border-b border-bd-0 px-3 py-2">
        <div className="mr-auto flex min-w-0 items-center gap-3">
          <span className="font-sans text-sm font-strong text-tx-0">{t('workspace.stage_graph')}</span>
          <span className="font-sans text-xs text-tx-3">
            {t('workspace.graph_stats', stats)}
          </span>
        </div>
        <button
          type="button"
          onClick={() => setDock(dock === 'validation' ? 'closed' : 'validation')}
          className={cn(
            'inline-flex h-8 items-center gap-1.5 rounded px-2 font-sans text-xs font-strong transition-colors',
            errorCount > 0
              ? 'bg-red-dim text-red-soft hover:bg-red/15'
              : issues.length > 0
                ? 'bg-yellow-dim text-yellow hover:bg-yellow/15'
                : 'bg-green-dim text-green-soft hover:bg-green/15',
          )}
        >
          {issues.length > 0
            ? <CircleAlert className="h-3.5 w-3.5" />
            : <CheckCircle2 className="h-3.5 w-3.5" />}
          {issues.length > 0
            ? t('graph.validation.summary', { count: issues.length })
            : t('graph.validation.ok')}
        </button>
        <span className="mx-0.5 h-5 w-px bg-bd-1" aria-hidden />
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <ChromeButton
              size="sm"
              disabled={readOnly}
              disabledReason={readOnlyReason}
            >
              <Plus className="h-3.5 w-3.5" /> {t('workspace.add_node')}
            </ChromeButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="min-w-44">
            <DropdownMenuItem onSelect={addSource}>
              <Database className="h-3.5 w-3.5 text-blue-soft" />
              {t('graph.add_source')}
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={addTransform}>
              <Filter className="h-3.5 w-3.5 text-indigo-soft" />
              {t('graph.add_transform')}
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={addSink}>
              <Send className="h-3.5 w-3.5 text-green-soft" />
              {t('graph.add_sink')}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <ChromeButton
          size="sm"
          onClick={() => setLayoutRequest((request) => request + 1)}
          title={t('workspace.auto_layout')}
        >
          <LayoutGrid className="h-3.5 w-3.5" />
          <span className="hidden 2xl:inline">{t('workspace.auto_layout')}</span>
        </ChromeButton>
        <ChromeButton
          size="sm"
          onClick={() => setInspectorOpen((open) => !open)}
          title={inspectorOpen ? t('graph.hide_inspector') : t('graph.show_inspector')}
        >
          {inspectorOpen
            ? <PanelRightClose className="h-3.5 w-3.5" />
            : <PanelRightOpen className="h-3.5 w-3.5" />}
          <span className="hidden 2xl:inline">
            {inspectorOpen ? t('graph.hide_inspector') : t('graph.show_inspector')}
          </span>
        </ChromeButton>
      </div>
      <div
        className={cn(
          'grid min-h-[500px] flex-1 grid-cols-1',
          inspectorOpen && 'lg:grid-cols-[minmax(0,1fr)_360px]',
        )}
      >
        <div className={cn('min-h-[500px] border-b border-bd-0 lg:border-b-0', inspectorOpen && 'lg:border-r')}>
          <PipelineGraphCanvas
            model={value}
            layoutRequest={layoutRequest}
            {...(inspectorOpen ? { selectedId } : {})}
            onSelectNode={(id) => {
              setSelectedId(id);
              setInspectorOpen(true);
            }}
          />
        </div>
        {inspectorOpen && (
          <fieldset
            disabled={readOnly}
            aria-disabled={readOnly || undefined}
            className="min-h-0 overflow-y-auto border-0 bg-bg-1 p-0 disabled:bg-bg-2"
          >
            <GraphInspector
              model={value}
              selectedId={selectedId}
              onChange={(next) => {
                if (!readOnly) onChange(next);
              }}
              onOpenCodeEditor={() => {
                if (!readOnly) setDock('code');
              }}
            />
          </fieldset>
        )}
      </div>
      <div className="border-t border-bd-0 bg-bg-1">
        <div className="flex min-h-9 items-center gap-1 px-2">
          <button
            type="button"
            onClick={() => setDock(dock === 'validation' ? 'closed' : 'validation')}
            className={cn(
              'inline-flex h-8 items-center gap-2 rounded px-2.5 font-sans text-xs font-strong text-tx-2 hover:bg-bg-3 hover:text-tx-0',
              dock === 'validation' && 'bg-bg-3 text-tx-0',
            )}
          >
            <CheckCircle2 className="h-3.5 w-3.5" />
            {t('workspace.validation_results')}
            <span className="font-mono text-tx-3">{issues.length}</span>
          </button>
          {selectedTransform && (
            <button
              type="button"
              onClick={() => setDock(dock === 'code' ? 'closed' : 'code')}
              className={cn(
                'inline-flex h-8 items-center gap-2 rounded px-2.5 font-sans text-xs font-strong text-tx-2 hover:bg-bg-3 hover:text-tx-0',
                dock === 'code' && 'bg-bg-3 text-tx-0',
              )}
            >
              <Braces className="h-3.5 w-3.5" />
              {t('workspace.code_editor')}
              <span className="max-w-40 truncate font-mono text-tx-3">
                {selectedTransform.name || t('graph.transform')}
              </span>
            </button>
          )}
          {dock !== 'closed' && (
            <button
              type="button"
              className="ml-auto grid h-7 w-7 place-items-center rounded text-tx-3 hover:bg-bg-3 hover:text-tx-0"
              onClick={() => setDock('closed')}
              aria-label={t('workspace.close_panel')}
              title={t('workspace.close_panel')}
            >
              <X className="h-3.5 w-3.5" />
            </button>
          )}
        </div>
        {dock === 'validation' && (
          <div className="h-[260px] overflow-auto border-t border-bd-0 bg-bg-2 p-4">
            {issues.length === 0 ? (
              <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
                <span className="grid h-9 w-9 place-items-center rounded-full bg-green-dim text-green-soft">
                  <CheckCircle2 className="h-5 w-5" />
                </span>
                <div className="font-sans text-sm font-strong text-tx-0">
                  {t('workspace.validation_clean')}
                </div>
                <div className="max-w-md font-sans text-xs leading-relaxed text-tx-3">
                  {t('workspace.validation_clean_hint')}
                </div>
              </div>
            ) : (
              <ul className="mx-auto flex max-w-3xl flex-col gap-2">
                {issues.map((issue, index) => (
                  <li
                    key={`${issue.code}-${index}`}
                    className="flex items-start gap-3 rounded-md border border-bd-0 bg-bg-1 px-3 py-2.5 font-sans text-xs"
                  >
                    <CircleAlert
                      className={cn(
                        'mt-0.5 h-3.5 w-3.5 shrink-0',
                        issue.level === 'error' ? 'text-red-soft' : 'text-yellow',
                      )}
                    />
                    <span className={issue.level === 'error' ? 'text-red-soft' : 'text-tx-1'}>
                      {t(`graph.validation.${issue.code}`, issue.params ?? {})}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
        {dock === 'code' && selectedTransform && (
          <div className="h-[300px] overflow-auto border-t border-bd-0 bg-bg-2 p-3">
            <CodeEditor
              value={selectedTransform.script}
              onChange={(script) => updateSelectedTransform({ script })}
              readOnly={readOnly}
              language="vrl"
              label={`${t('workspace.code_editor')} · ${selectedTransform.name || t('graph.transform')}`}
              ariaLabel={t('graph.vrl_script')}
              minHeight={220}
              maxHeight={260}
              resizable
              onModSave={() => setDock('closed')}
            />
          </div>
        )}
      </div>
    </div>
  );
}

function PipelineGraphCanvas({
  model,
  selectedId,
  onSelectNode,
  layoutRequest = 0,
}: {
  model: PipelineGraphModel;
  selectedId?: string;
  onSelectNode?: (id: string) => void;
  layoutRequest?: number;
}) {
  const instanceRef = React.useRef<ReactFlowInstance | null>(null);
  const connectorsQuery = useQuery({ queryKey: ['connectors'], queryFn: () => connectorsApi.list() });
  const connectorNames = React.useMemo(() => {
    const map: Record<string, string> = {};
    for (const connector of connectorsQuery.data ?? []) map[connector.id] = connector.name;
    return map;
  }, [connectorsQuery.data]);
  const built = React.useMemo(
    () => buildElements(model, selectedId, connectorNames),
    [model, selectedId, connectorNames],
  );

  const [nodes, setNodes, onNodesChange] = useNodesState(built.nodes);
  const [edges, setEdges, onEdgesChange] = useEdgesState(built.edges);
  const builtRef = React.useRef(built);
  builtRef.current = built;

  // 结构变化（增删节点 / 重命名）时按 id 重新播种，保留用户已拖动的坐标。
  // 不含选中态 - 纯点选由 React Flow 内部处理，避免重播种把用户刚画的连线重置掉。
  const structureKey = React.useMemo(
    () => built.nodes.map((node) => `${node.id}:${node.data.label}`).join('|'),
    [built.nodes],
  );
  React.useEffect(() => {
    setNodes((prev) => {
      const positions = new Map(prev.map((node) => [node.id, node.position]));
      return built.nodes.map((node) => ({ ...node, position: positions.get(node.id) ?? node.position }));
    });
    setEdges(built.edges);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [structureKey]);

  React.useEffect(() => {
    if (layoutRequest === 0) return;
    const next = builtRef.current;
    setNodes(next.nodes);
    setEdges(next.edges);
    const frame = window.requestAnimationFrame(() => {
      void instanceRef.current?.fitView({ padding: 0.16, maxZoom: 1 });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [layoutRequest, setEdges, setNodes]);

  const onConnect = React.useCallback(
    (conn: Connection) => {
      if (!isValidGraphConnection(conn)) return;
      setEdges((current) =>
        addEdge(
          { ...conn, animated: true, style: { stroke: 'var(--indigo)', strokeWidth: 2 } },
          current,
        ),
      );
    },
    [setEdges],
  );

  // 只读预览（无 onSelectNode）保持静态；编辑态开启拖拽 / 连线 / 选择。
  const interactive = Boolean(onSelectNode);
  return (
    <ReactFlow
      nodes={nodes}
      edges={edges}
      onNodesChange={onNodesChange}
      onEdgesChange={onEdgesChange}
      onConnect={onConnect}
      isValidConnection={isValidGraphConnection}
      nodeTypes={NODE_TYPES}
      nodesDraggable={interactive}
      nodesConnectable={interactive}
      elementsSelectable={interactive}
      onInit={(instance) => {
        instanceRef.current = instance;
      }}
      fitView
      fitViewOptions={{ padding: 0.16, maxZoom: 1 }}
      minZoom={0.3}
      onNodeClick={(_, node) => onSelectNode?.(node.id)}
      proOptions={{ hideAttribution: true }}
    >
      <Background variant={BackgroundVariant.Dots} gap={16} color="var(--bd-0)" />
      <Controls showInteractive={false} className="!border-bd-0 !bg-bg-1" />
    </ReactFlow>
  );
}

function PipelineGraphNode({ data, selected }: NodeProps<GraphNodeData>) {
  const { t } = useTranslation('pipelines');
  const Icon = data.kind === 'source' ? Database : data.kind === 'sink' ? Send : Filter;
  return (
    <div
      className={cn(
        'relative min-h-[72px] w-[220px] rounded-md border bg-bg-1 px-3 py-3 shadow-sm transition-colors',
        selected
          ? 'border-indigo bg-indigo-dim shadow-[0_0_0_1px_var(--indigo)]'
          : 'border-bd-1 hover:border-bd-2',
      )}
    >
      {data.kind !== 'source' && (
        <Handle type="target" position={Position.Left} className="!h-3 !w-3 !border-2 !border-bg-1 !bg-indigo" />
      )}
      <div className="flex items-center gap-2">
        <span className="grid h-7 w-7 shrink-0 place-items-center rounded-md border border-bd-0 bg-bg-2 text-blue-soft">
          <Icon className="h-4 w-4" />
        </span>
        <div className="min-w-0 flex-1">
          <div className="truncate font-sans text-xs font-semibold text-tx-0">{data.label}</div>
          <div className="mt-1 truncate font-sans text-xs text-tx-3">{t(data.subtitleKey)}</div>
        </div>
        <Pill tone={data.tone}>{t(`graph.node_kinds.${data.kind}`)}</Pill>
      </div>
      {data.kind !== 'sink' && (
        <Handle type="source" position={Position.Right} className="!h-3 !w-3 !border-2 !border-bg-1 !bg-indigo" />
      )}
    </div>
  );
}

function GraphInspector({
  model,
  selectedId,
  onChange,
  onOpenCodeEditor,
}: {
  model: PipelineGraphModel;
  selectedId: string;
  onChange: (next: PipelineGraphModel) => void;
  onOpenCodeEditor: () => void;
}) {
  const { t } = useTranslation('pipelines');
  const connectorsQuery = useQuery({ queryKey: ['connectors'], queryFn: () => connectorsApi.list() });
  const connectors = connectorsQuery.data ?? [];
  const streamsQuery = useQuery({
    queryKey: ['streams', 'pipeline-options'],
    queryFn: () => streamsApi.list(500),
  });
  const streams = streamsQuery.data ?? [];
  const selected = parseNodeId(selectedId);

  if (!selected) {
    return (
      <GraphInspectorPanel title={t('workspace.node_configuration')}>
        <div className="p-4 font-sans text-xs text-tx-3">{t('graph.select_node')}</div>
      </GraphInspectorPanel>
    );
  }

  if (selected.kind === 'source') {
    const value = model.sources[selected.index] ?? '';
    const canDelete = model.sources.length > 1;
    const streamOptions = pipelineStreamOptions(streams, model.signalType, value, (name) =>
      t('graph.stream_current_value', { name }),
    );
    return (
      <GraphInspectorPanel
        title={t('workspace.node_configuration')}
        nodeKind={t('graph.node_kinds.source')}
        nodeName={value}
      >
        <div className="flex flex-col gap-4 p-4">
          <FormField label={t('graph.source_name')}>
            <FormSelect
              value={value}
              onChange={(next) => {
                const sources = model.sources.map((item, index) =>
                  index === selected.index ? next : item,
                );
                onChange({ ...model, sources });
              }}
              options={streamOptions}
              placeholder={
                streamsQuery.isPending
                  ? t('graph.stream_loading')
                  : t('graph.stream_select_placeholder')
              }
              className="bg-bg-1"
            />
            {streamsQuery.isError ? (
              <p className="mt-1 font-sans text-xs text-red-soft">
                {t('graph.stream_load_error')}
              </p>
            ) : null}
            {!streamsQuery.isPending && !streamsQuery.isError && streamOptions.length === 0 ? (
              <p className="mt-1 font-sans text-xs text-tx-3">
                {t('graph.stream_empty')}
              </p>
            ) : null}
          </FormField>
          <ChromeButton
            disabled={!canDelete}
            className="justify-center border-red text-red-soft disabled:cursor-not-allowed disabled:opacity-50"
            onClick={() => {
              if (!canDelete) return;
              onChange({
                ...model,
                sources: model.sources.filter((_, index) => index !== selected.index),
              });
            }}
          >
            <Trash2 className="h-3.5 w-3.5" /> {t('graph.delete_node')}
          </ChromeButton>
        </div>
      </GraphInspectorPanel>
    );
  }

  if (selected.kind === 'sink') {
    const value = model.sinks[selected.index] ?? '';
    const canDelete = model.sinks.length > 1;
    const connectorId = value.startsWith('connector:') ? value.slice('connector:'.length) : '';
    const streamOptions = pipelineStreamOptions(streams, model.signalType, value, (name) =>
      t('graph.stream_current_value', { name }),
    );
    const setSink = (next: string) => {
      const sinks = model.sinks.map((item, index) => (index === selected.index ? next : item));
      onChange({ ...model, sinks });
    };
    return (
      <GraphInspectorPanel
        title={t('workspace.node_configuration')}
        nodeKind={t('graph.node_kinds.sink')}
        nodeName={value}
      >
        <div className="flex flex-col gap-4 p-4">
          <FormField label={t('graph.sink_target')} hint={t('graph.sink_target_hint')}>
            <FormSelect
              value={connectorId ? `connector:${connectorId}` : '__stream'}
              onChange={(next) => {
                if (next === '__new') window.open('/pipelines/connectors', '_blank', 'noopener');
                else if (next === '__stream') setSink('');
                else setSink(next);
              }}
              options={[
                { value: '__stream', label: t('graph.sink_stream_option') },
                ...connectors.map((connector) => ({
                  value: `connector:${connector.id}`,
                  label: `${connector.name} (${connector.kind})`,
                })),
                { value: '__new', label: t('graph.new_connector') },
              ]}
              className="bg-bg-1"
            />
          </FormField>
          {!connectorId && (
            <FormField label={t('graph.sink_name')}>
              <FormSelect
                value={value}
                onChange={setSink}
                options={streamOptions}
                placeholder={
                  streamsQuery.isPending
                    ? t('graph.stream_loading')
                    : t('graph.stream_select_placeholder')
                }
                className="bg-bg-1"
              />
              {streamsQuery.isError ? (
                <p className="mt-1 font-sans text-xs text-red-soft">
                  {t('graph.stream_load_error')}
                </p>
              ) : null}
              {!streamsQuery.isPending && !streamsQuery.isError && streamOptions.length === 0 ? (
                <p className="mt-1 font-sans text-xs text-tx-3">
                  {t('graph.stream_empty')}
                </p>
              ) : null}
            </FormField>
          )}
          <ChromeButton
            disabled={!canDelete}
            className="justify-center border-red text-red-soft disabled:cursor-not-allowed disabled:opacity-50"
            onClick={() => {
              if (!canDelete) return;
              onChange({
                ...model,
                sinks: model.sinks.filter((_, index) => index !== selected.index),
              });
            }}
          >
            <Trash2 className="h-3.5 w-3.5" /> {t('graph.delete_node')}
          </ChromeButton>
        </div>
      </GraphInspectorPanel>
    );
  }

  // transform 步骤：编辑该步的名称与 VRL 函数；retry policy 是流水线级共享设置。
  const transform = model.transforms[selected.index] ?? { name: '', script: '' };
  const canDelete = model.transforms.length > 1;
  const setTransform = (patch: Partial<TransformStep>) => {
    const transforms = model.transforms.map((item, index) =>
      index === selected.index ? { ...item, ...patch } : item,
    );
    onChange({ ...model, transforms });
  };
  return (
    <GraphInspectorPanel
      title={t('workspace.node_configuration')}
      nodeKind={t('graph.node_kinds.transform')}
      nodeName={transform.name || t('graph.transform')}
    >
      <div className="flex flex-col gap-4 p-4">
        <FormField label={t('graph.transform_name')}>
          <FormInput value={transform.name} onChange={(event) => setTransform({ name: event.target.value })} />
        </FormField>
        <FormField label={t('graph.retry_policy')}>
          <FormSelect
            value={model.retryPolicy}
            onChange={(retryPolicy) => onChange({ ...model, retryPolicy })}
            options={[
              { value: 'exponential', label: t('drawer.retry_options.exponential') },
              { value: 'fixed', label: t('drawer.retry_options.fixed') },
              { value: 'none', label: t('drawer.retry_options.none') },
            ]}
          />
        </FormField>
        <VrlFunctionField
          script={transform.script}
          onPick={(script) => setTransform({ script })}
          onOpenEditor={onOpenCodeEditor}
        />
        <ChromeButton
          disabled={!canDelete}
          className="justify-center border-red text-red-soft disabled:cursor-not-allowed disabled:opacity-50"
          onClick={() => {
            if (!canDelete) return;
            onChange({
              ...model,
              transforms: model.transforms.filter((_, index) => index !== selected.index),
            });
          }}
        >
          <Trash2 className="h-3.5 w-3.5" /> {t('graph.delete_node')}
        </ChromeButton>
      </div>
    </GraphInspectorPanel>
  );
}

export function pipelineStreamOptions(
  streams: streamsApi.StreamSummary[],
  signalType: PipelineSignalType,
  currentValue = '',
  currentLabel: (name: string) => string = (name) => name,
): Array<{ value: string; label: string }> {
  const options = streams
    .filter((stream) => stream.stream_type === signalType)
    .map((stream) => ({ value: stream.name, label: stream.name }))
    .sort((left, right) => left.label.localeCompare(right.label));
  if (
    currentValue &&
    !currentValue.startsWith('connector:') &&
    !options.some((option) => option.value === currentValue)
  ) {
    options.unshift({ value: currentValue, label: currentLabel(currentValue) });
  }
  return options;
}

function GraphInspectorPanel({
  title,
  nodeKind,
  nodeName,
  children,
}: {
  title: React.ReactNode;
  nodeKind?: React.ReactNode;
  nodeName?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <aside className="min-h-full bg-bg-1">
      <div className="border-b border-bd-0 px-4 py-3">
        <div className="font-sans text-sm font-strong text-tx-0">{title}</div>
        {nodeKind && (
          <div className="mt-1 flex min-w-0 items-center gap-1.5 font-sans text-xs text-tx-3">
            <span>{nodeKind}</span>
            <span aria-hidden>·</span>
            <span className="truncate font-mono text-tx-2">{nodeName}</span>
          </div>
        )}
      </div>
      {children}
    </aside>
  );
}

/**
 * VRL 转换字段：把内联脚本编辑器换成「选择已保存的 VRL 函数」+「新建」按钮。
 * 流水线仍只持久化内联 script，选中函数即把其 source 灌入 script；新建走 functions
 * 整页路由，用新标签页打开以保住未保存的流水线编辑状态。
 */
function VrlFunctionField({
  script,
  onPick,
  onOpenEditor,
}: {
  script: string;
  onPick: (source: string) => void;
  onOpenEditor: () => void;
}) {
  const { t } = useTranslation('pipelines');
  const q = useQuery({ queryKey: ['functions'], queryFn: () => functionsApi.list() });
  const vrlFns = React.useMemo(
    () => (q.data ?? []).filter((fn) => fn.language === 'vrl'),
    [q.data],
  );

  // functions.list 已含内置预设（is_builtin）。按 source 回显当前选中；不命中留空（自定义/未选）。
  // 选项值用函数 id（避免内置与同名自建函数撞名时解析歧义）。
  const selected = vrlFns.find((fn) => fn.source === script);
  const options = vrlFns.map((fn) => ({
    value: fn.id,
    label: fn.is_builtin ? t('graph.preset_label', { name: fn.name }) : fn.name,
  }));

  const handlePick = (id: string) => {
    const fn = vrlFns.find((item) => item.id === id);
    if (fn) onPick(fn.source);
  };

  return (
    <FormField label={t('graph.vrl_script')} hint={t('graph.vrl_function_hint')}>
      <div className="flex items-center gap-2">
        <FormSelect
          value={selected?.id ?? ''}
          onChange={handlePick}
          options={options}
          placeholder={t('graph.vrl_function_placeholder')}
          className="flex-1 bg-bg-1"
        />
        <ChromeButton
          type="button"
          aria-label={t('graph.new_vrl_function')}
          title={t('graph.new_vrl_function')}
          onClick={() => window.open('/functions/new', '_blank', 'noopener')}
        >
          <Plus className="h-3 w-3" />
        </ChromeButton>
      </div>
      {selected?.is_builtin && selected.description ? (
        <p className="mt-1 font-sans text-xs text-tx-3">{selected.description}</p>
      ) : null}
      <ChromeButton type="button" className="mt-2 w-full justify-center" onClick={onOpenEditor}>
        <Braces className="h-3.5 w-3.5" />
        {t('workspace.open_editor')}
      </ChromeButton>
    </FormField>
  );
}

function buildElements(
  model: PipelineGraphModel,
  selectedId?: string,
  connectorNames: Record<string, string> = {},
): { nodes: Node<GraphNodeData>[]; edges: Edge[] } {
  const sourceTone = SIGNAL_TONE[model.signalType];
  const defaults = DEFAULT_GRAPH[model.signalType];

  const sources = normalizedList(model.sources, defaults.sources);
  const sinks = normalizedList(model.sinks, defaults.sinks);
  const transforms = model.transforms.length > 0
    ? model.transforms
    : [{ name: defaults.transformName, script: DEFAULT_VRL_SCRIPT }];

  // 从左到右编排：来源 → 转换链 → 目标。同类来源/目标纵向排开，完整三节点流程在
  // 280–360px 高的概览画布里仍能以接近 1:1 的比例展示。
  const colGap = 320;
  const rowGap = 128;
  const transformStartX = colGap;
  const sinkX = colGap * (transforms.length + 1);
  const lastTransformId = `transform-${transforms.length - 1}`;

  const nodes: Node<GraphNodeData>[] = [
    ...sources.map((source, index) => ({
      id: `source-${index}`,
      type: 'pipeline',
      position: { x: 0, y: index * rowGap },
      selected: selectedId === `source-${index}`,
      data: {
        kind: 'source' as const,
        label: source,
        subtitleKey: 'graph.node_subtitles.stream_input',
        tone: sourceTone,
      },
    })),
    ...transforms.map((transform, index) => ({
      id: `transform-${index}`,
      type: 'pipeline',
      position: { x: transformStartX + index * colGap, y: 0 },
      selected: selectedId === `transform-${index}`,
      data: {
        kind: 'transform' as const,
        label: transform.name.trim() || defaults.transformName,
        subtitleKey: 'graph.node_subtitles.vrl_transform',
        tone: 'blue' as const,
      },
    })),
    ...sinks.map((sink, index) => {
      const isConnector = sink.startsWith('connector:');
      const connectorId = isConnector ? sink.slice('connector:'.length) : '';
      return {
        id: `sink-${index}`,
        type: 'pipeline',
        position: { x: sinkX, y: index * rowGap },
        selected: selectedId === `sink-${index}`,
        data: {
          kind: 'sink' as const,
          label: isConnector ? (connectorNames[connectorId] ?? connectorId) : sink,
          subtitleKey: isConnector
            ? 'graph.node_subtitles.connector_sink'
            : 'graph.node_subtitles.stream_output',
          tone: 'green' as const,
        },
      };
    }),
  ];

  const edges: Edge[] = [
    ...sources.map((_, index) => ({
      id: `source-${index}->transform-0`,
      source: `source-${index}`,
      target: 'transform-0',
      animated: true,
      style: { stroke: 'var(--indigo)', strokeWidth: 2 },
    })),
    ...transforms.slice(1).map((_, index) => ({
      id: `transform-${index}->transform-${index + 1}`,
      source: `transform-${index}`,
      target: `transform-${index + 1}`,
      animated: true,
      style: { stroke: 'var(--indigo)', strokeWidth: 2 },
    })),
    ...sinks.map((_, index) => ({
      id: `${lastTransformId}->sink-${index}`,
      source: lastTransformId,
      target: `sink-${index}`,
      animated: true,
      style: { stroke: 'var(--indigo)', strokeWidth: 2 },
    })),
  ];

  return { nodes, edges };
}

type ParsedNode =
  | { kind: 'source'; index: number }
  | { kind: 'sink'; index: number }
  | { kind: 'transform'; index: number };

function parseNodeId(id: string): ParsedNode | null {
  const match = /^(source|sink|transform)-(\d+)$/.exec(id);
  if (!match) return null;
  return { kind: match[1] as 'source' | 'sink' | 'transform', index: Number(match[2]) };
}

function stepObject(value: unknown): Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function stringsFrom(value: unknown, fallback?: string | string[]): string[] {
  if (Array.isArray(value)) return value.map(String).map((item) => item.trim()).filter(Boolean);
  if (typeof value === 'string' && value.trim()) return [value.trim()];
  if (Array.isArray(fallback)) return fallback;
  return fallback ? [fallback] : [];
}

function normalizedList(value: string[], fallback: string[]): string[] {
  const normalized = unique(value.map((item) => item.trim()).filter(Boolean));
  return normalized.length > 0 ? normalized : fallback;
}

function unique(value: string[]): string[] {
  return [...new Set(value)];
}

function firstString(obj: Record<string, unknown>, keys: string[]): string {
  for (const key of keys) {
    const candidate = obj[key];
    if (typeof candidate === 'string' && candidate.trim()) return candidate.trim();
  }
  return '';
}

/**
 * 把 function_steps 解析成处理步骤列表。兼容三种历史/当前形态：
 * 新结构 `{ steps: [{transform_name, script}] }`、裸数组 `[{...}]`、以及旧单对象
 * `{ transform_name, script }`。任何形态都至少产出一个步骤。
 */
function transformsFromSteps(value: unknown, fallbackName: string): TransformStep[] {
  const obj = stepObject(value);
  const rawSteps: unknown[] | null = Array.isArray(value)
    ? value
    : Array.isArray(obj.steps)
      ? (obj.steps as unknown[])
      : null;
  if (rawSteps) {
    const parsed: TransformStep[] = [];
    for (const step of rawSteps) {
      const so = stepObject(step);
      const name = firstString(so, ['transform_name', 'function_name', 'name']);
      const script = typeof so.script === 'string' ? so.script : '';
      if (!name && !script) continue;
      parsed.push({ name: name || fallbackName, script: script || DEFAULT_VRL_SCRIPT });
    }
    if (parsed.length > 0) return parsed;
  }
  if ('script' in obj || 'transform_name' in obj || 'function_name' in obj) {
    return [
      {
        name: firstString(obj, ['transform_name', 'function_name', 'name']) || fallbackName,
        script: typeof obj.script === 'string' ? obj.script : DEFAULT_VRL_SCRIPT,
      },
    ];
  }
  if (typeof value === 'string' && value.trim()) {
    return [{ name: fallbackName, script: value }];
  }
  return [{ name: fallbackName, script: DEFAULT_VRL_SCRIPT }];
}
