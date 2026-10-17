import { AxiosError, type AxiosResponse } from 'axios';
import { describe, expect, it, vi } from 'vitest';

import { defaultPipelineGraph, type PipelineGraphModel, type Routing } from '../../PipelineGraph';
import { describeFailure, routeDestination, runPipelineTest } from './run';

function apiFailure(status: number, message: string): AxiosError {
  return new AxiosError(
    `Request failed with status code ${status}`,
    AxiosError.ERR_BAD_REQUEST,
    undefined,
    undefined,
    { status, data: { message } } as AxiosResponse,
  );
}

const route = (routing: Partial<Routing>, target = 'default') => ({
  name: 'route',
  kind: 'builtin' as const,
  operation: 'route' as const,
  script: '',
  target,
  routing: { kind: 'fixed' as const, field: '', prefix: '', fallback: 'default', retain_source: false, ...routing },
});

function graph(overrides: Partial<PipelineGraphModel>): PipelineGraphModel {
  return { ...defaultPipelineGraph('logs'), ...overrides };
}

describe('runPipelineTest', () => {
  const passed = (output: unknown) => ({ output, dropped: false });

  it('hands each step what the one before left', async () => {
    const runScript = vi.fn(async (script: string, input: unknown) =>
      passed({ ...(input as object), [script]: true }),
    );

    const outcome = await runPipelineTest(
      graph({
        sources: ['app'],
        sinks: ['out', 'connector:archive', ' '],
        transforms: [
          { name: 'first', script: 'a' },
          { name: 'second', script: 'b' },
        ],
      }),
      { n: 1 },
      runScript,
    );

    // Each step sees the stream the event was read from, the first source unless another is said.
    expect(runScript.mock.calls).toEqual([
      ['a', { n: 1 }, 'app'],
      ['b', { n: 1, a: true }, 'app'],
    ]);
    expect(outcome).toEqual({
      steps: [
        { index: 0, name: 'first', status: 'passed', output: { n: 1, a: true } },
        { index: 1, name: 'second', status: 'passed', output: { n: 1, a: true, b: true } },
      ],
      output: { n: 1, a: true, b: true },
      // Every destination that is named, connectors included.
      destinations: ['out', 'connector:archive'],
    });
  });

  it('stops at the step that fails, as a run does, and writes nothing', async () => {
    const runScript = vi
      .fn()
      .mockResolvedValueOnce(passed({ ok: 1 }))
      .mockRejectedValueOnce(apiFailure(500, 'internal error'));

    const outcome = await runPipelineTest(
      graph({
        transforms: [
          { name: 'first', script: 'a' },
          { name: 'second', script: 'b' },
          { name: 'third', script: 'c' },
        ],
      }),
      {},
      runScript,
    );

    expect(outcome.steps.map((step) => step.status)).toEqual(['passed', 'failed', 'skipped']);
    expect(outcome.steps[1]?.failure).toEqual({ kind: 'runtime', message: '' });
    expect(outcome.output).toBeUndefined();
    expect(outcome.destinations).toEqual([]);
    expect(runScript).toHaveBeenCalledTimes(2);
  });

  it('stops at the step that drops the event, which then goes nowhere', async () => {
    const runScript = vi
      .fn()
      .mockResolvedValueOnce(passed({ level: 'debug', tagged: true }))
      .mockResolvedValueOnce({ output: { level: 'debug', tagged: true }, dropped: true });

    const outcome = await runPipelineTest(
      graph({
        sinks: ['out', 'connector:c1'],
        transforms: [
          { name: 'tag', script: 'a' },
          { name: 'quiet', script: 'b' },
          { name: 'after', script: 'c' },
        ],
      }),
      { level: 'debug' },
      runScript,
    );

    expect(outcome.steps).toEqual([
      { index: 0, name: 'tag', status: 'passed', output: { level: 'debug', tagged: true } },
      // As it came to the step that dropped it.
      { index: 1, name: 'quiet', status: 'dropped', output: { level: 'debug', tagged: true } },
      { index: 2, name: 'after', status: 'skipped' },
    ]);
    expect(outcome.output).toBeUndefined();
    expect(outcome.destinations).toEqual([]);
    expect(runScript).toHaveBeenCalledTimes(2);
  });

  it('runs the steps on the event as read from the source it is said to come from', async () => {
    const runScript = vi.fn(async (_script: string, input: unknown) => passed(input));

    await runPipelineTest(
      graph({ sources: ['app', 'nginx'], transforms: [{ name: 'tag', script: 'a' }] }),
      {},
      runScript,
      'nginx',
    );

    expect(runScript).toHaveBeenCalledWith('a', {}, 'nginx');
  });

  it('lets a routing step choose the stream from the event as it is at that point', async () => {
    const outcome = await runPipelineTest(
      graph({
        sinks: ['ignored'],
        transforms: [
          { name: 'tag', script: 'tag' },
          route({ kind: 'field', field: 'appname', prefix: 'app_' }),
          { name: 'rewrite', script: 'rewrite' },
        ],
      }),
      {},
      async (script) => passed(script === 'tag' ? { appname: 'orders' } : { appname: 'changed' }),
    );

    expect(outcome.steps[1]).toEqual({
      index: 1,
      name: 'route',
      status: 'passed',
      output: { appname: 'orders' },
      destination: 'app_orders',
    });
    expect(outcome.destinations).toEqual(['app_orders']);
    expect(outcome.output).toEqual({ appname: 'changed' });
  });

  it('writes every stream and delivers to every connector', async () => {
    const model = graph({ sinks: ['clean', 'connector:c1', 'archive', 'connector:c2', 'connector:'] });
    const nothing = async () => passed({});

    expect((await runPipelineTest(model, {}, nothing)).destinations).toEqual([
      'clean',
      'archive',
      'connector:c1',
      'connector:c2',
    ]);
    // A routing step changes the first stream, not who else gets the event.
    expect(
      (await runPipelineTest({ ...model, transforms: [route({}, 'orders')] }, {}, nothing)).destinations,
    ).toEqual(['orders', 'archive', 'connector:c1', 'connector:c2']);
    // Routed into one of the other streams, the event is written there once.
    expect(
      (await runPipelineTest({ ...model, transforms: [route({}, 'archive')] }, {}, nothing)).destinations,
    ).toEqual(['archive', 'connector:c1', 'connector:c2']);
    // A realtime pipeline writes to its stream and nowhere else.
    expect((await runPipelineTest({ ...model, mode: 'realtime' }, {}, nothing)).destinations).toEqual([
      'clean',
    ]);
  });

  it('gives the event back as it came when there are no steps', async () => {
    const runScript = vi.fn();
    const outcome = await runPipelineTest(graph({ sinks: ['copy'], transforms: [] }), { n: 1 }, runScript);

    expect(outcome).toEqual({ steps: [], output: { n: 1 }, destinations: ['copy'] });
    expect(runScript).not.toHaveBeenCalled();
  });
});

// The cases of the backend's own test for `Routing::destination`.
describe('routeDestination', () => {
  const byField: Routing = {
    kind: 'field',
    field: 'service.name',
    prefix: '',
    fallback: 'default',
    retain_source: false,
  };

  it('sends everything to the fixed stream', () => {
    expect(routeDestination({ ...byField, kind: 'fixed' }, 'orders', { 'service.name': 'x' })).toBe(
      'orders',
    );
  });

  it('prefers the key as written, then the path into the event', () => {
    expect(
      routeDestination(byField, 'unused', {
        'service.name': 'literal',
        service: { name: 'nested' },
      }),
    ).toBe('literal');
    expect(routeDestination(byField, 'unused', { service: { name: 'nested' } })).toBe('nested');
  });

  it('falls back for a value that is not a stream name', () => {
    for (const value of [null, 42, true, '../bad', '_molesignal', '']) {
      expect(routeDestination(byField, 'unused', { 'service.name': value })).toBe('default');
    }
    expect(routeDestination(byField, 'unused', {})).toBe('default');
    expect(routeDestination(byField, 'unused', 'not an object')).toBe('default');
  });

  it('puts the prefix before the value', () => {
    expect(routeDestination({ ...byField, prefix: 'logs_' }, 'unused', { 'service.name': 'orders' })).toBe(
      'logs_orders',
    );
  });
});

describe('describeFailure', () => {
  it('reads the message and what it points at out of a compile error', () => {
    const dump =
      'invalid argument: vrl compile: DiagnosticList([Diagnostic { severity: Error, code: 103, message: "unhandled fallible assignment", labels: [Label { message: "this expression is fallible because at least one argument\'s type cannot be verified to be valid", primary: true, span: Span { start: 5, end: 15 } }, Label { message: "or change this to an infallible assignment:", primary: false, span: Span { start: 0, end: 4 } }], notes: [] }])';

    expect(describeFailure(apiFailure(400, dump))).toEqual({
      kind: 'compile',
      message:
        "unhandled fallible assignment: this expression is fallible because at least one argument's type cannot be verified to be valid",
    });
  });

  it('unquotes what the dump escaped', () => {
    const dump =
      'invalid argument: vrl compile: DiagnosticList([Diagnostic { severity: Error, code: 701, message: "call to undefined variable", labels: [Label { message: "did you mean \\"null\\"?", primary: true, span: Span { start: 1, end: 2 } }], notes: [] }])';

    expect(describeFailure(apiFailure(400, dump)).message).toBe(
      'call to undefined variable: did you mean "null"?',
    );
  });

  it('keeps a compile error it cannot read as it came', () => {
    expect(describeFailure(apiFailure(400, 'invalid argument: vrl compile: ???'))).toEqual({
      kind: 'compile',
      message: 'invalid argument: vrl compile: ???',
    });
  });

  it('has nothing to add to a script that failed on the event', () => {
    expect(describeFailure(apiFailure(500, 'internal error'))).toEqual({ kind: 'runtime', message: '' });
  });

  it('passes any other refusal on', () => {
    expect(describeFailure(apiFailure(403, 'forbidden: functions.run'))).toEqual({
      kind: 'request',
      message: 'forbidden: functions.run',
    });
  });
});
