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

/** How often what failed is tried again. */
export interface RetrySettings {
  /** More attempts at a window whose read or write failed, after a backoff, before it is skipped. */
  runRetries: number;
  /** More attempts at a delivery to a connector within the run, before its events become dead letters. */
  deliveryRetries: number;
}

/** What the backend uses when a pipeline leaves these out (`domain/pipeline/scheduled.rs`). */
export const DEFAULT_RETRY: RetrySettings = { runRetries: 3, deliveryRetries: 3 };
export const DEFAULT_DELAY_SECS = 60;
export const MAX_RETRIES = 10;
export const MAX_DELAY_SECS = 3_600;
/** At most this many streams on either side of a pipeline. */
export const MAX_STREAMS = 8;

/**
 * A pipeline as the editor holds it:
 *  - every stream in `sources` is read; a realtime pipeline reads one.
 *  - `sinks` are where events go. Every stream there takes every event, except
 *    that a routing step decides where the first one's events go (the first
 *    stream stands for the routed output then). `connector:<id>` entries are
 *    delivered to; the id is empty until a connector is chosen.
 *  - `transforms` run in order and may be none: the events are then written
 *    as they were read.
 *  - the rest says how a scheduled pipeline reads and what it does when
 *    something fails.
 */
export interface PipelineGraphModel {
  mode?: 'scheduled' | 'realtime';
  retainSource?: boolean;
  signalType: PipelineSignalType;
  sources: string[];
  sinks: string[];
  transforms: TransformStep[];
  retry: RetrySettings;
  /** The logs stream that events which could not be processed, written or delivered go to; blank for none. */
  deadLetterStream: string;
  /** Within a run, an event whose value of this field was already seen is dropped; blank for none. */
  dedupeField: string;
  /** How long a run leaves the latest stretch of time for late events before it reads it. */
  delaySecs: number;
}

export const CONNECTOR_PREFIX = 'connector:';

/** The connector a destination delivers to; `null` for a stream. */
export function connectorOf(sink: string): string | null {
  return sink.startsWith(CONNECTOR_PREFIX) ? sink.slice(CONNECTOR_PREFIX.length) : null;
}

/** Where in `sinks` the first stream sits, the one a routing step decides for; -1 when there is none. */
export function firstStreamIndex(sinks: readonly string[]): number {
  return sinks.findIndex((sink) => connectorOf(sink) === null);
}

/** Whether a routing step decides where events go. */
export function isRouted(model: Pick<PipelineGraphModel, 'transforms'>): boolean {
  return model.transforms.some((step) => step.kind === 'builtin');
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

  const retry = stepObject(steps.retry);

  return {
    mode: executionMode(pipeline),
    retainSource: typeof steps.retain_source === 'boolean' ? steps.retain_source : stepObject(steps.routing).retain_source === true,
    signalType,
    // A pipeline that names no stream yet has the place for one, to be chosen.
    sources: sources.length > 0 ? sources : [''],
    sinks: [...(streamSinks.length > 0 ? streamSinks : ['']), ...connectorSinks],
    transforms: processingStepsFromPipeline(pipeline),
    retry: {
      runRetries: wholeNumber(retry.run_retries, DEFAULT_RETRY.runRetries, MAX_RETRIES),
      deliveryRetries: wholeNumber(retry.delivery_retries, DEFAULT_RETRY.deliveryRetries, MAX_RETRIES),
    },
    deadLetterStream: firstString(steps, ['dead_letter_stream']),
    dedupeField: firstString(steps, ['dedupe_field']),
    delaySecs: wholeNumber(steps.delay_secs, DEFAULT_DELAY_SECS, MAX_DELAY_SECS),
  };
}

/** The streams a saved pipeline reads and writes, its own first: what a list names it by. */
export function pipelineStreams(pipeline: ScheduledPipeline): { sources: string[]; sinks: string[] } {
  const steps = stepObject(pipeline.function_steps);

  return {
    sources: unique([...stringsFrom(pipeline.source_stream), ...stringsFrom(steps.sources)]),
    sinks: unique([...stringsFrom(pipeline.target_stream), ...stringsFrom(steps.sinks)]),
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
  const scheduled = graph.mode !== 'realtime';
  const sources = named(graph.sources);
  const streams = graph.sinks.filter((sink) => connectorOf(sink) === null);
  const route = graph.transforms.filter((step) => step.kind === 'builtin').at(-1);
  const routeTarget = route ? (route.routing?.kind === 'field' ? route.routing.fallback : route.target) : undefined;
  // The first stream is where a routing step's events go when it picks none; the rest take copies.
  const streamSinks = named([routeTarget ?? streams[0] ?? '', ...streams.slice(1)]);
  const connectorSinks = unique(graph.sinks.map(connectorOf).filter((id): id is string => Boolean(id)));
  const deadLetterStream = graph.deadLetterStream.trim();
  const dedupeField = graph.dedupeField.trim();

  return {
    name,
    source_stream: sources[0] ?? '',
    target_stream: streamSinks[0] ?? '',
    function_steps: {
      mode: graph.mode ?? 'scheduled',
      ...(graph.mode === 'realtime' && { retain_source: graph.retainSource ?? false }),
      signal_type: graph.signalType,
      sources,
      sinks: streamSinks,
      sink_connectors: connectorSinks,
      ...(scheduled && {
        delay_secs: graph.delaySecs,
        retry: { run_retries: graph.retry.runRetries, delivery_retries: graph.retry.deliveryRetries },
        ...(deadLetterStream && { dead_letter_stream: deadLetterStream }),
        ...(dedupeField && { dedupe_field: dedupeField }),
      }),
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

/** A whole number from 0 to `max` as saved, or `fallback` for anything else. */
function wholeNumber(value: unknown, fallback: number, max: number): number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= max ? value : fallback;
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
