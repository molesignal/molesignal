import type { PipelineInput, ScheduledPipeline } from '@/api/pipelines';

import {
  compileProcessor,
  parseProcessor,
  type ProcessorConfig,
  type ProcessorOp,
} from '../processors/catalog';

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
  /**
   * The no-code processor this step's `script` was generated from. The backend
   * runs the script and keeps this beside it, so the step opens as a form again.
   */
  processor?: ProcessorConfig;
}

/** What a step is, as the editor offers it: routing, a processor, or VRL written by hand. */
export type StepType = 'route' | 'vrl' | ProcessorOp;

export function stepType(step: TransformStep): StepType {
  if (step.kind === 'builtin') return 'route';
  return step.processor?.op ?? 'vrl';
}

/** The processor of a step, while its script is still the one the processor generates. */
function processorOf(step: TransformStep): ProcessorConfig | undefined {
  return step.processor && compileProcessor(step.processor) === step.script
    ? step.processor
    : undefined;
}

/**
 * A pipeline as the editor holds it. What runs is narrower than the lists
 * suggest, and the editor shows it that way:
 *  - `sources[0]` is the stream that is read. A pipeline reads one stream; any
 *    name after it was saved by an earlier editor and is never read.
 *  - The first entry of `sinks` that is not a connector is the stream that is
 *    written (unless a routing step picks the stream per event). Later stream
 *    names are likewise never written. `connector:<id>` entries are all
 *    delivered to; the id is empty until a connector is chosen.
 *  - `transforms` run in order and may be none: the events are then written
 *    as they were read.
 */
export interface PipelineGraphModel {
  mode?: 'scheduled' | 'realtime';
  retainSource?: boolean;
  signalType: PipelineSignalType;
  sources: string[];
  sinks: string[];
  transforms: TransformStep[];
  /** Kept as it was saved. Nothing reads it, so the editor does not offer it. */
  retryPolicy: string;
}

export const CONNECTOR_PREFIX = 'connector:';

/** The connector a destination delivers to; `null` for a stream. */
export function connectorOf(sink: string): string | null {
  return sink.startsWith(CONNECTOR_PREFIX) ? sink.slice(CONNECTOR_PREFIX.length) : null;
}

/** Where in `sinks` the stream that is written sits; -1 when every destination is a connector. */
export function writtenSinkIndex(sinks: readonly string[]): number {
  return sinks.findIndex((sink) => connectorOf(sink) === null);
}


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
  const steps = stepObject(pipeline?.function_steps);
  // What is read and written is the pipeline's own source and target. The lists
  // beside the steps repeat them, so those two lead whatever order the lists were saved in.
  const sources = unique([...stringsFrom(pipeline?.source_stream), ...stringsFrom(steps.sources)]);
  const streamSinks = unique([...stringsFrom(pipeline?.target_stream), ...stringsFrom(steps.sinks)]);
  const connectorSinks = unique(stringsFrom(steps.sink_connectors)).map((id) => `${CONNECTOR_PREFIX}${id}`);

  return {
    mode: executionMode(pipeline),
    retainSource: typeof steps.retain_source === 'boolean' ? steps.retain_source : stepObject(steps.routing).retain_source === true,
    signalType,
    // A pipeline that names no stream yet has the place for one, to be chosen.
    sources: sources.length > 0 ? sources : [''],
    sinks: [...(streamSinks.length > 0 ? streamSinks : ['']), ...connectorSinks],
    transforms: processingStepsFromPipeline(pipeline),
    retryPolicy: typeof steps.retry_policy === 'string' ? steps.retry_policy : 'exponential',
  };
}

/** A pipeline nobody has filled in yet: a source and a destination to choose, and no steps. */
export function defaultPipelineGraph(signalType: PipelineSignalType = 'logs'): PipelineGraphModel {
  return pipelineGraphFromPipeline(null, signalType);
}

/**
 * The pipeline as the backend takes it. Nothing is made up here: a flow that
 * lacks a source, a destination or a script is held back by `validateGraph`
 * before it gets this far.
 */
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
  const sources = named(graph.sources);
  const sinks = named(graph.sinks);
  const route = graph.transforms.filter((step) => step.kind === 'builtin').at(-1);
  const routeTarget = route ? (route.routing?.kind === 'field' ? route.routing.fallback : route.target) : undefined;
  const streamSinks = routeTarget ? [routeTarget] : sinks.filter((sink) => connectorOf(sink) === null);
  const connectorSinks = sinks.map(connectorOf).filter((id): id is string => Boolean(id));
  const targetStream = streamSinks[0] ?? '';

  return {
    name,
    source_stream: sources[0] ?? '',
    target_stream: targetStream,
    function_steps: {
      mode: graph.mode ?? 'scheduled',
      ...(graph.mode === 'realtime' && { retain_source: graph.retainSource ?? false }),
      signal_type: graph.signalType,
      sources,
      sinks: streamSinks,
      sink_connectors: connectorSinks,
      retry_policy: graph.retryPolicy || 'exponential',
      steps: graph.transforms.map((step, index) => ({
        transform_name: step.name.trim() || `step-${index + 1}`,
        kind: step.kind ?? 'vrl',
        ...(step.kind === 'builtin' ? {
          operation: 'route',
          target: step.target ?? 'default',
          routing: routeConfig(step.routing ?? defaultRouting),
        } : {
          script: step.script,
          ...(processorOf(step) && { processor: processorOf(step) }),
        }),
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

/** The names in a list that are names: trimmed, without blanks or repeats. */
function named(value: string[]): string[] {
  return unique(value.map((item) => item.trim()).filter(Boolean));
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
 * `{ transform_name, script }`。Nothing saved means no steps: the editor shows what runs.
 */
function transformsFromSteps(value: unknown): TransformStep[] {
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
      // The script is what runs. A stored form is kept only while it still generates it.
      const processor = parseProcessor(so.processor);
      parsed.push({
        name,
        script,
        ...(processor && compileProcessor(processor) === script ? { processor } : {}),
      });
    }
    return parsed;
  }
  if ('script' in obj || 'transform_name' in obj || 'function_name' in obj) {
    return [
      {
        name: firstString(obj, ['transform_name', 'function_name', 'name']),
        script: typeof obj.script === 'string' ? obj.script : '',
      },
    ];
  }
  if (typeof value === 'string' && value.trim()) {
    return [{ name: '', script: value }];
  }
  return [];
}

function routeConfig(routing: Routing) {
  return { kind: routing.kind, field: routing.field, prefix: routing.prefix, fallback: routing.fallback };
}

/** Routing an earlier editor saved beside the steps, as the routing step it is now. */
function withLegacyRouting(steps: TransformStep[], routing: Routing | undefined, target: string): TransformStep[] {
  return routing ? [...steps, { name: 'route', kind: 'builtin', operation: 'route', script: '', target, routing }] : steps;
}

function processingStepsFromPipeline(pipeline: ScheduledPipeline | null | undefined): TransformStep[] {
  const config = stepObject(pipeline?.function_steps);
  const steps = transformsFromSteps(pipeline?.function_steps);
  return withLegacyRouting(steps, config.routing ? { ...defaultRouting, ...stepObject(config.routing) } as Routing : undefined, pipeline?.target_stream ?? 'default');
}
