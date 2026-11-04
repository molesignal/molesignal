import type { PipelineInput, ScheduledPipeline } from '@/api/pipelines';

import {
  alertConditions,
  alertStepInput,
  DEFAULT_SUMMARY_ANNOTATION,
  enrichQueryOf,
  type AlertConditions,
  type EnrichQuerySettings,
} from './alertModel';
import { processorOf, stepObject, stringsFrom, wholeNumber } from './savedValues';
import { isRumStream } from './streamName';
import {
  countingInput,
  countingOf,
  isCountingKind,
  type CountingKind,
  type CountingParts,
} from '../counting/model';
import {
  compileProcessor,
  type ProcessorConfig,
  type ProcessorOp,
} from '../processors/catalog';
import { parseProcessor } from '../processors/parse';

export * from './alertModel';

/** Profiles are their metadata rows, and only as they arrive: a scheduled pipeline does not read them. */
export type PipelineSignalType = 'logs' | 'metrics' | 'traces' | 'profiles';

/**
 * When a pipeline runs: on stored data at an interval, on logs as they arrive,
 * or on an alert event.
 */
export type PipelineMode = 'scheduled' | 'realtime' | 'alert';

export interface Routing {
  kind: 'fixed' | 'field';
  field: string;
  prefix: string;
  fallback: string;
  retainSource: boolean;
}
export const defaultRouting: Routing = { kind: 'fixed', field: 'appname', prefix: '', fallback: 'default', retainSource: false };
export function executionMode(pipeline: ScheduledPipeline | null | undefined): PipelineMode {
  const mode = stepObject(pipeline?.functionSteps).mode;
  return mode === 'realtime' || mode === 'alert' ? mode : 'scheduled';
}

/** 单个 VRL 处理步骤（按顺序串行应用）。 */
export interface TransformStep extends CountingParts {
  name: string;
  script: string;
  /**
   * An alert pipeline's steps that go outside MoleSignal, when an incident opens: `ai_summary` asks
   * the organization's model, `http_enrich` calls an HTTP connector, `query_enrich` runs a query.
   * A realtime pipeline's steps that keep count across events are a `CountingKind`.
   */
  kind?: 'vrl' | 'builtin' | 'ai_summary' | 'http_enrich' | 'query_enrich' | CountingKind;
  /** What an AI summary step asks the model. */
  prompt?: string;
  /** The annotation an AI summary step writes its answer into. */
  annotation?: string;
  /** The HTTP connector an HTTP enrich step calls. */
  connectorId?: string;
  /** What a query enrich step runs. */
  query?: EnrichQuerySettings;
  operation?: 'route';
  routing?: Routing;
  target?: string;
  /**
   * The no-code processor this step's `script` was generated from. The backend
   * runs the script and keeps this beside it, so the step opens as a form again.
   */
  processor?: ProcessorConfig;
}

/**
 * What a step is, as the editor offers it: routing, a processor, VRL written by hand, a step that
 * goes outside, or one that keeps count.
 */
export type StepType = 'route' | 'vrl' | 'ai_summary' | 'http_enrich' | 'query_enrich' | CountingKind | ProcessorOp;

export function stepType(step: TransformStep): StepType {
  if (step.kind === 'builtin') return 'route';
  if (step.kind === 'ai_summary' || step.kind === 'http_enrich' || step.kind === 'query_enrich') return step.kind;
  if (isCountingKind(step.kind)) return step.kind;
  return step.processor?.op ?? 'vrl';
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
  mode?: PipelineMode;
  /** What an alert pipeline is for; kept while another mode is shown, saved only for an alert pipeline. */
  alert: AlertConditions;
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
export const TABLE_PREFIX = 'table:';

/** The connector a destination delivers to; `null` for anything else. */
export function connectorOf(sink: string): string | null {
  return sink.startsWith(CONNECTOR_PREFIX) ? sink.slice(CONNECTOR_PREFIX.length) : null;
}

/** The extend table a destination writes rows into; `null` for anything else. */
export function tableOf(sink: string): string | null {
  return sink.startsWith(TABLE_PREFIX) ? sink.slice(TABLE_PREFIX.length) : null;
}

/** Whether a destination is a stream, not a connector or an extend table. */
export function isStreamSink(sink: string): boolean {
  return connectorOf(sink) === null && tableOf(sink) === null;
}

/** Where in `sinks` the first stream sits, the one a routing step decides for; -1 when there is none. */
export function firstStreamIndex(sinks: readonly string[]): number {
  return sinks.findIndex(isStreamSink);
}

/** Whether a routing step decides where events go. */
export function isRouted(model: Pick<PipelineGraphModel, 'transforms'>): boolean {
  return model.transforms.some((step) => step.kind === 'builtin');
}

/**
 * Whether the pipeline changes metrics, spans or RUM events as they arrive:
 * they stay in the stream they arrive in, so its one destination is its source.
 */
export function changesInPlace(model: Pick<PipelineGraphModel, 'mode' | 'signalType' | 'sources'>): boolean {
  return model.mode === 'realtime' && (model.signalType !== 'logs' || isRumStream(model.sources[0]?.trim() ?? ''));
}


export function signalTypeFromPipeline(
  pipeline: ScheduledPipeline | null | undefined,
  fallback: PipelineSignalType = 'logs',
): PipelineSignalType {
  const steps = stepObject(pipeline?.functionSteps);
  if (
    steps.signalType === 'logs' ||
    steps.signalType === 'metrics' ||
    steps.signalType === 'traces' ||
    steps.signalType === 'profiles'
  ) {
    return steps.signalType;
  }
  const candidates = [
    steps.signalType,
    pipeline?.description,
    pipeline?.sourceStream,
    pipeline?.targetStream,
    pipeline?.name,
    JSON.stringify(pipeline?.functionSteps ?? ''),
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
  const steps = stepObject(pipeline?.functionSteps);
  // What is read and written is the pipeline's own source and target. The lists
  // beside the steps repeat them, so those two lead whatever order the lists were saved in.
  const sources = unique([...stringsFrom(pipeline?.sourceStream), ...stringsFrom(steps.sources)]);
  const streamSinks = unique([...stringsFrom(pipeline?.targetStream), ...stringsFrom(steps.sinks)]);
  const connectorSinks = unique(stringsFrom(steps.sinkConnectors)).map((id) => `${CONNECTOR_PREFIX}${id}`);
  const tableSinks = unique(stringsFrom(steps.sinkTables)).map((name) => `${TABLE_PREFIX}${name}`);
  const mode = executionMode(pipeline);

  const retry = stepObject(steps.retry);

  return {
    mode,
    alert: alertConditions(steps.alert),
    retainSource: typeof steps.retainSource === 'boolean' ? steps.retainSource : stepObject(steps.routing).retainSource === true,
    signalType,
    // A pipeline that names no stream yet has the place for one, to be chosen. An alert pipeline
    // reads alert events and writes them back: it has no stream on either side.
    sources: mode === 'alert' ? [] : sources.length > 0 ? sources : [''],
    sinks:
      mode === 'alert'
        ? []
        : [...(streamSinks.length > 0 ? streamSinks : ['']), ...connectorSinks, ...tableSinks],
    transforms: processingStepsFromPipeline(pipeline),
    retry: {
      runRetries: wholeNumber(retry.runRetries, DEFAULT_RETRY.runRetries, MAX_RETRIES),
      deliveryRetries: wholeNumber(retry.deliveryRetries, DEFAULT_RETRY.deliveryRetries, MAX_RETRIES),
    },
    deadLetterStream: firstString(steps, ['deadLetterStream']),
    dedupeField: firstString(steps, ['dedupeField']),
    delaySecs: wholeNumber(steps.delaySecs, DEFAULT_DELAY_SECS, MAX_DELAY_SECS),
  };
}

/** The streams a saved pipeline reads and writes, its own first: what a list names it by. */
export function pipelineStreams(pipeline: ScheduledPipeline): { sources: string[]; sinks: string[] } {
  const steps = stepObject(pipeline.functionSteps);

  return {
    sources: unique([...stringsFrom(pipeline.sourceStream), ...stringsFrom(steps.sources)]),
    sinks: unique([...stringsFrom(pipeline.targetStream), ...stringsFrom(steps.sinks)]),
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
  if (graph.mode === 'alert') return alertPipelineInput({ name, graph, enabled });

  const scheduled = graph.mode !== 'realtime';
  const inPlace = changesInPlace(graph);
  const sources = named(graph.sources);
  const streams = graph.sinks.filter(isStreamSink);
  const route = graph.transforms.filter((step) => step.kind === 'builtin').at(-1);
  const routeTarget = route ? (route.routing?.kind === 'field' ? route.routing.fallback : route.target) : undefined;
  // The first stream is where a routing step's events go when it picks none; the rest take copies.
  // Metrics and spans changed as they arrive go back to the stream they came from.
  const streamSinks = inPlace ? sources.slice(0, 1) : named([routeTarget ?? streams[0] ?? '', ...streams.slice(1)]);
  const connectorSinks = unique(graph.sinks.map(connectorOf).filter((id): id is string => Boolean(id)));
  const tableSinks = unique(graph.sinks.map(tableOf).filter((table): table is string => Boolean(table)));
  const deadLetterStream = graph.deadLetterStream.trim();
  const dedupeField = graph.dedupeField.trim();

  return {
    name,
    sourceStream: sources[0] ?? '',
    targetStream: streamSinks[0] ?? '',
    functionSteps: {
      mode: graph.mode ?? 'scheduled',
      ...(graph.mode === 'realtime' && { retainSource: !inPlace && (graph.retainSource ?? false) }),
      signalType: graph.signalType,
      sources,
      sinks: streamSinks,
      sinkConnectors: connectorSinks,
      ...(scheduled && {
        sinkTables: tableSinks,
        delaySecs: graph.delaySecs,
        retry: { runRetries: graph.retry.runRetries, deliveryRetries: graph.retry.deliveryRetries },
        ...(deadLetterStream && { deadLetterStream }),
        ...(dedupeField && { dedupeField }),
      }),
      steps: graph.transforms.map((step, index) => ({
        transformName: step.name.trim() || `step-${index + 1}`,
        kind: step.kind ?? 'vrl',
        ...(isCountingKind(step.kind) ? countingInput(step.kind, step) : step.kind === 'builtin' ? {
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
    ...(lookbackSecs !== undefined && { lookbackSecs: lookbackSecs }),
    ...(enabled !== undefined && { enabled }),
  };
}


/**
 * An alert pipeline as the backend takes it: its conditions and steps. It
 * reads and writes no stream and runs on no schedule.
 */
function alertPipelineInput({
  name,
  graph,
  enabled,
}: {
  name: string;
  graph: PipelineGraphModel;
  enabled?: boolean | undefined;
}): PipelineInput {
  const labels = graph.alert.labels.filter((label) => label.key.trim() !== '');

  return {
    name,
    sourceStream: '',
    targetStream: '',
    functionSteps: {
      mode: 'alert',
      alert: {
        stage: graph.alert.stage,
        ...(graph.alert.stage === 'notify' && { policies: graph.alert.policies }),
        // Only a pipeline before notifications sees incidents opened by hand.
        sources:
          graph.alert.stage === 'notify'
            ? graph.alert.sources
            : graph.alert.sources.filter((source) => source !== 'manual'),
        severities: graph.alert.severities,
        labels: Object.fromEntries(labels.map((label) => [label.key.trim(), label.value])),
        ...(graph.alert.time && { time: graph.alert.time }),
        order: graph.alert.order,
      },
      steps: graph.transforms.map((step, index) => ({
        transformName: step.name.trim() || `step-${index + 1}`,
        ...alertStepInput(step),
      })),
    },
    cron: '',
    ...(enabled !== undefined && { enabled }),
  };
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
 * 新结构 `{ steps: [{transformName, script}] }`、裸数组 `[{...}]`、以及旧单对象
 * `{ transformName, script }`。Nothing saved means no steps: the editor shows what runs.
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
      const name = firstString(so, ['transformName', 'function_name', 'name']);
      if (so.kind === 'ai_summary') {
        parsed.push({
          name,
          kind: 'ai_summary',
          script: '',
          prompt: typeof so.prompt === 'string' ? so.prompt : '',
          annotation: typeof so.annotation === 'string' ? so.annotation : DEFAULT_SUMMARY_ANNOTATION,
        });
        continue;
      }
      if (so.kind === 'http_enrich') {
        parsed.push({
          name,
          kind: 'http_enrich',
          script: '',
          connectorId: typeof so.connectorId === 'string' ? so.connectorId : '',
        });
        continue;
      }
      if (so.kind === 'query_enrich') {
        parsed.push({ name, kind: 'query_enrich', script: '', query: enrichQueryOf(so) });
        continue;
      }
      if (isCountingKind(so.kind)) {
        parsed.push({ name, kind: so.kind, script: '', ...countingOf(so.kind, so) });
        continue;
      }
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
  if ('script' in obj || 'transformName' in obj || 'function_name' in obj) {
    return [
      {
        name: firstString(obj, ['transformName', 'function_name', 'name']),
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
  const config = stepObject(pipeline?.functionSteps);
  const steps = transformsFromSteps(pipeline?.functionSteps);
  return withLegacyRouting(steps, config.routing ? { ...defaultRouting, ...stepObject(config.routing) } as Routing : undefined, pipeline?.targetStream ?? 'default');
}
