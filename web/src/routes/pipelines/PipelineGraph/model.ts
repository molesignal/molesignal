import type { PipelineInput, ScheduledPipeline } from '@/api/pipelines';

export type PipelineSignalType = 'logs' | 'metrics' | 'traces';

export interface Routing {
  kind: 'fixed' | 'field';
  field: string;
  prefix: string;
  fallback: string;
  retain_source: boolean;
}
export const defaultRouting: Routing = { kind: 'fixed', field: 'appname', prefix: '', fallback: 'default', retain_source: false };
export function executionMode(pipeline: ScheduledPipeline | null | undefined): 'realtime' | 'scheduled' {
  return stepObject(pipeline?.function_steps).mode === 'realtime' ? 'realtime' : 'scheduled';
}

/** 单个 VRL 处理步骤（按顺序串行应用）。 */
export interface TransformStep {
  name: string;
  script: string;
  kind?: 'vrl' | 'builtin';
  operation?: 'route';
  routing?: Routing;
  target?: string;
}

export interface PipelineGraphModel {
  mode?: 'scheduled' | 'realtime';
  routing?: Routing;
  retainSource?: boolean;
  signalType: PipelineSignalType;
  sources: string[];
  sinks: string[];
  transforms: TransformStep[];
  retryPolicy: string;
}


export const DEFAULT_VRL_SCRIPT = `. = parse_json!(.message)

.environment = "production"
.cluster = "us-east-1"
.level = downcase(.level || "info")

if exists(.trace_id) {
    .trace.id = .trace_id
    del(.trace_id)
}`;


export const DEFAULT_GRAPH: Record<
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


export function signalTypeFromPipeline(
  pipeline: ScheduledPipeline | null | undefined,
  fallback: PipelineSignalType = 'logs',
): PipelineSignalType {
  const steps = stepObject(pipeline?.function_steps);
  if (steps.signal_type === 'logs' || steps.signal_type === 'metrics' || steps.signal_type === 'traces') return steps.signal_type;
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
    mode: executionMode(pipeline),
    retainSource: typeof steps.retain_source === 'boolean' ? steps.retain_source : stepObject(steps.routing).retain_source === true,
    signalType,
    sources: unique(stepSources.length > 0 ? stepSources : stringsFrom(pipeline?.source_stream, defaults.sources)),
    sinks: unique([
      ...(stepSinks.length > 0 ? stepSinks : stringsFrom(pipeline?.target_stream, defaults.sinks)),
      ...connectorSinks,
    ]),
    transforms: processingStepsFromPipeline(pipeline, defaults.transformName),
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
  const route = graph.transforms.filter((step) => step.kind === 'builtin').at(-1);
  const routeTarget = route ? (route.routing?.kind === 'field' ? route.routing.fallback : route.target) : undefined;
  const streamSinks = routeTarget ? [routeTarget] : allSinks.filter((sink) => !sink.startsWith('connector:'));
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
      mode: graph.mode ?? 'scheduled',
      ...(graph.mode === 'realtime' && { retain_source: graph.retainSource ?? graph.routing?.retain_source ?? false }),
      signal_type: graph.signalType,
      sources,
      sinks: streamSinks,
      sink_connectors: connectorSinks,
      retry_policy: graph.retryPolicy || 'exponential',
      steps: withLegacyRouting(transforms, graph.routing, targetStream).map((step) => ({
        transform_name: step.name.trim() ? step.name.trim() : defaults.transformName,
        kind: step.kind ?? 'vrl',
        ...(step.kind === 'builtin' ? {
          operation: 'route',
          target: step.target ?? 'default',
          routing: routeConfig(step.routing ?? defaultRouting),
        } : { script: step.script.trim() ? step.script : DEFAULT_VRL_SCRIPT }),
      })),
    },
    cron: graph.mode === 'realtime' ? '' : cron,
    ...(lookbackSecs !== undefined && { lookback_secs: lookbackSecs }),
    ...(enabled !== undefined && { enabled }),
  };
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

export function normalizedList(value: string[], fallback: string[]): string[] {
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
      if (so.kind === 'builtin') {
        parsed.push({ name: name || 'route', kind: 'builtin', operation: 'route', script: '', target: typeof so.target === 'string' ? so.target : 'default', routing: { ...defaultRouting, ...stepObject(so.routing) } as Routing });
        continue;
      }
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

function routeConfig(routing: Routing) {
  return { kind: routing.kind, field: routing.field, prefix: routing.prefix, fallback: routing.fallback };
}

function withLegacyRouting(steps: TransformStep[], routing: Routing | undefined, target: string): TransformStep[] {
  return routing ? [...steps, { name: 'route', kind: 'builtin', operation: 'route', script: '', target, routing }] : steps;
}

function processingStepsFromPipeline(pipeline: ScheduledPipeline | null | undefined, fallbackName: string): TransformStep[] {
  const config = stepObject(pipeline?.function_steps);
  const steps = executionMode(pipeline) === 'realtime' && Array.isArray(config.steps) && config.steps.length === 0
    ? [{ name: 'process', script: '. = .' }]
    : transformsFromSteps(pipeline?.function_steps, fallbackName);
  return withLegacyRouting(steps, config.routing ? { ...defaultRouting, ...stepObject(config.routing) } as Routing : undefined, pipeline?.target_stream ?? 'default');
}
