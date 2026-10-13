import { toApiError } from '@/lib/http';

import {
  connectorOf,
  defaultRouting,
  type PipelineGraphModel,
  type Routing,
} from '../../PipelineGraph/model';
import { isStreamName } from '../../PipelineGraph/streamName';

export interface StepOutcome {
  index: number;
  name: string;
  status: 'passed' | 'failed' | 'skipped';
  /** The event as the step left it. */
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
  /** The event as it would be written; absent when a step failed, since a failed event is not written. */
  output?: unknown;
  /** Where the event would go: the one stream it is written to, then the connectors it is delivered to. */
  destinations: string[];
}

/** Runs one VRL script on one event and gives the event back. */
export type RunScript = (source: string, input: unknown) => Promise<unknown>;

/**
 * Takes one event through a pipeline's steps the way a run does: in order,
 * each on what the one before left, a routing step choosing the stream. A
 * step that fails ends it; the steps after are not tried.
 */
export async function runPipelineTest(
  model: PipelineGraphModel,
  input: unknown,
  runScript: RunScript,
): Promise<TestOutcome> {
  const steps: StepOutcome[] = [];
  let event = input;
  let routed: string | undefined;
  let failed = false;

  for (const [index, step] of model.transforms.entries()) {
    const name = step.name.trim();

    if (failed) {
      steps.push({ index, name, status: 'skipped' });
      continue;
    }

    if (step.kind === 'builtin') {
      routed = routeDestination(step.routing ?? defaultRouting, step.target ?? 'default', event);
      steps.push({ index, name, status: 'passed', output: event, destination: routed });
      continue;
    }

    try {
      event = await runScript(step.script, event);
      steps.push({ index, name, status: 'passed', output: event });
    } catch (error) {
      failed = true;
      steps.push({ index, name, status: 'failed', failure: describeFailure(error) });
    }
  }

  if (failed) return { steps, destinations: [] };

  // One stream is written: the one a routing step chose, or the pipeline's own. Connectors get
  // the event either way; a realtime pipeline has none.
  const stream = routed ?? model.sinks.find((sink) => connectorOf(sink) === null)?.trim();
  const connectors = model.mode === 'realtime' ? [] : model.sinks.filter((sink) => Boolean(connectorOf(sink)));

  return { steps, output: event, destinations: [...(stream ? [stream] : []), ...connectors] };
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
