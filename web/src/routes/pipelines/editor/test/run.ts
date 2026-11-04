import { toApiError } from '@/lib/http';

import { countingOutcome, type CountingOutcome } from '../../counting/evaluate';
import { isCountingKind } from '../../counting/model';
import {
  changesInPlace,
  DEFAULT_QUERY_ANNOTATION,
  DEFAULT_SUMMARY_ANNOTATION,
  defaultRouting,
  firstStreamIndex,
  isOwnAlertLabel,
  isStreamSink,
  type PipelineGraphModel,
  type Routing,
} from '../../PipelineGraph/model';
import { changedProtectedField } from '../../PipelineGraph/protectedFields';
import { isStreamName } from '../../PipelineGraph/streamName';

export interface StepOutcome {
  index: number;
  name: string;
  /** `dropped`: the step let the event go on purpose (VRL `abort`); it goes no further. */
  status: 'passed' | 'failed' | 'dropped' | 'skipped';
  /** The event as the step left it; for a step that dropped it, as it came to the step. */
  output?: unknown;
  /** Where a routing step sends the event. */
  destination?: string;
  /** The step goes outside MoleSignal, which a test does not do: it passed without being run. */
  outside?: boolean;
  /** What a step that keeps count would count the event as; a test counts nothing for real. */
  counting?: CountingOutcome;
  failure?: TestFailure;
}

export interface TestFailure {
  /**
   * `compile`: the script is not valid VRL. `runtime`: it is, and failed on
   * this event. `protected`: it changed a field the pipeline keeps as it
   * arrives, named by `message`, which fails the event when it is written.
   */
  kind: 'compile' | 'runtime' | 'request' | 'protected';
  message: string;
}

export interface TestOutcome {
  steps: StepOutcome[];
  /** The event as it would be written; absent when a step failed or dropped it, since it is not written then. */
  output?: unknown;
  /**
   * Where the event would go: the stream a routing step chose or the first
   * stream, every other stream, then the connectors it is delivered to.
   */
  destinations: string[];
}

/** What running one VRL script on one event did. */
export interface ScriptResult {
  output: unknown;
  /** The script dropped the event on purpose (`abort`). */
  dropped: boolean;
}

/** Runs one VRL script on one event read from `source`, which the script sees as `%source_stream`. */
export type RunScript = (script: string, input: unknown, source: string) => Promise<ScriptResult>;

/**
 * What an AI summary step puts into its annotation in a test: the model is
 * asked only about a real event, when its incident opens.
 */
export const SUMMARY_PLACEHOLDER = '…';

/**
 * Takes one event, as read from `source`, through a pipeline's steps the way
 * a run does: in order, each on what the one before left, a routing step
 * choosing the first stream. A step that fails or drops the event ends it;
 * the steps after are not tried.
 */
export async function runPipelineTest(
  model: PipelineGraphModel,
  input: unknown,
  runScript: RunScript,
  source = model.sources[0]?.trim() ?? '',
): Promise<TestOutcome> {
  const steps: StepOutcome[] = [];
  let event = input;
  let routed: string | undefined;
  let ended = false;

  for (const [index, step] of model.transforms.entries()) {
    const name = step.name.trim();

    if (ended) {
      steps.push({ index, name, status: 'skipped' });
      continue;
    }

    if (step.kind === 'builtin') {
      routed = routeDestination(step.routing ?? defaultRouting, step.target ?? 'default', event);
      steps.push({ index, name, status: 'passed', output: event, destination: routed });
      continue;
    }

    // One event never goes past a limit: the step says what it would count it as and lets it on.
    if (isCountingKind(step.kind)) {
      steps.push({ index, name, status: 'passed', output: event, counting: countingOutcome(step.kind, step, event) });
      continue;
    }

    if (step.kind === 'ai_summary') {
      event = withAnnotation(event, step.annotation?.trim() || DEFAULT_SUMMARY_ANNOTATION, SUMMARY_PLACEHOLDER);
      steps.push({ index, name, status: 'passed', output: event });
      continue;
    }

    // A test calls nothing outside: a real event does that, when its incident opens.
    if (step.kind === 'http_enrich') {
      steps.push({ index, name, status: 'passed', output: event, outside: true });
      continue;
    }

    if (step.kind === 'query_enrich') {
      event = withAnnotation(event, step.query?.annotation.trim() || DEFAULT_QUERY_ANNOTATION, SUMMARY_PLACEHOLDER);
      steps.push({ index, name, status: 'passed', output: event, outside: true });
      continue;
    }

    try {
      const result = await runScript(step.script, event, source);

      const kept = changesInPlace(model)
        ? changedProtectedField(model.signalType, event, result.output, source)
        : null;

      if (result.dropped) {
        ended = true;
        steps.push({ index, name, status: 'dropped', output: event });
      } else if (kept) {
        ended = true;
        steps.push({ index, name, status: 'failed', failure: { kind: 'protected', message: kept } });
      } else {
        event = result.output;
        steps.push({ index, name, status: 'passed', output: event });
      }
    } catch (error) {
      ended = true;
      steps.push({ index, name, status: 'failed', failure: describeFailure(error) });
    }
  }

  if (ended) return { steps, destinations: [] };

  return {
    steps,
    output: model.mode === 'alert' ? withOwnLabels(input, event) : event,
    destinations: destinationsOf(model, routed),
  };
}

/** An alert event as it comes out: the labels MoleSignal set itself stay what they were. */
function withOwnLabels(input: unknown, output: unknown): unknown {
  if (!isRecord(output) || !isRecord(output.labels)) return output;

  const own = isRecord(input) && isRecord(input.labels) ? input.labels : {};

  return {
    ...output,
    labels: Object.fromEntries([
      ...Object.entries(output.labels).filter(([key]) => !isOwnAlertLabel(key)),
      ...Object.entries(own).filter(([key]) => isOwnAlertLabel(key)),
    ]),
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/**
 * The streams and connectors an event goes to: the routed or first stream,
 * the other streams, then the connectors. A realtime pipeline writes its one
 * stream only.
 */
function destinationsOf(model: PipelineGraphModel, routed: string | undefined): string[] {
  // Metrics and spans changed as they arrive go back into the stream they came from.
  if (changesInPlace(model)) return [model.sources[0]?.trim() ?? ''].filter(Boolean);

  const first = firstStreamIndex(model.sinks);
  const streams = model.sinks.flatMap((sink, index) => {
    if (!isStreamSink(sink)) return [];
    if (index === first) return [routed ?? sink.trim()];
    return model.mode === 'realtime' ? [] : [sink.trim()];
  });
  const others =
    model.mode === 'realtime'
      ? []
      : model.sinks.filter((sink) => !isStreamSink(sink) && sink !== 'connector:' && sink !== 'table:');

  return [...new Set([...streams.filter(Boolean), ...others])];
}

/** The event with `text` under the annotation `key`, as an AI summary step leaves it. */
function withAnnotation(event: unknown, key: string, text: string): unknown {
  if (typeof event !== 'object' || event === null || Array.isArray(event)) return event;

  const fields = event as Record<string, unknown>;
  const annotations =
    typeof fields.annotations === 'object' && fields.annotations !== null && !Array.isArray(fields.annotations)
      ? (fields.annotations as Record<string, unknown>)
      : {};

  return { ...fields, annotations: { ...annotations, [key]: text } };
}

/**
 * Where a routing step sends an event, as the backend decides it: the fixed
 * stream, or the prefix and the value of the field (the key as written first,
 * then as a path into the event), falling back when that is not a stream name.
 */
export function routeDestination(routing: Routing, fixed: string, event: unknown): string {
  if (routing.kind === 'fixed') return fixed;

  const value = lookup(event, routing.field);
  if (typeof value === 'string' && value !== '') {
    const stream = `${routing.prefix}${value}`;
    if (isStreamName(stream)) return stream;
  }

  return routing.fallback;
}

function lookup(event: unknown, field: string): unknown {
  const own = member(event, field);
  if (own !== undefined) return own;

  return field.split('.').reduce<unknown>((value, key) => member(value, key), event);
}

function member(value: unknown, key: string): unknown {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)[key]
    : undefined;
}

/**
 * What went wrong, in words. The backend reports a script that does not
 * compile as a debug dump of VRL's diagnostics; the first message and the
 * note on what it points at are what a person needs. A script that compiled
 * and failed on the event comes back as an internal error with no detail.
 */
export function describeFailure(error: unknown): TestFailure {
  const { status, message } = toApiError(error);

  if (message.includes('vrl compile')) {
    const headline = /message: "((?:[^"\\]|\\.)*)"/.exec(message)?.[1];
    const pointer = /Label \{ message: "((?:[^"\\]|\\.)*)", primary: true/.exec(message)?.[1];
    const text = [headline, pointer].filter(Boolean).map(unquote).join(': ');

    return { kind: 'compile', message: text || message };
  }
  if (status >= 500) return { kind: 'runtime', message: '' };

  return { kind: 'request', message };
}

function unquote(text: string | undefined): string {
  return (text ?? '').replace(/\\(["\\])/g, '$1');
}
