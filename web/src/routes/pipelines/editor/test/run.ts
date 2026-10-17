import { toApiError } from '@/lib/http';

import {
  connectorOf,
  defaultRouting,
  firstStreamIndex,
  type PipelineGraphModel,
  type Routing,
} from '../../PipelineGraph/model';
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
  failure?: TestFailure;
}

export interface TestFailure {
  /** `compile`: the script is not valid VRL. `runtime`: it is, and failed on this event. */
  kind: 'compile' | 'runtime' | 'request';
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

    try {
      const result = await runScript(step.script, event, source);

      if (result.dropped) {
        ended = true;
        steps.push({ index, name, status: 'dropped', output: event });
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

  return { steps, output: event, destinations: destinationsOf(model, routed) };
}

/**
 * The streams and connectors an event goes to: the routed or first stream,
 * the other streams, then the connectors. A realtime pipeline writes its one
 * stream only.
 */
function destinationsOf(model: PipelineGraphModel, routed: string | undefined): string[] {
  const first = firstStreamIndex(model.sinks);
  const streams = model.sinks.flatMap((sink, index) => {
    if (connectorOf(sink) !== null) return [];
    if (index === first) return [routed ?? sink.trim()];
    return model.mode === 'realtime' ? [] : [sink.trim()];
  });
  const connectors = model.mode === 'realtime' ? [] : model.sinks.filter((sink) => Boolean(connectorOf(sink)));

  return [...new Set([...streams.filter(Boolean), ...connectors])];
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
