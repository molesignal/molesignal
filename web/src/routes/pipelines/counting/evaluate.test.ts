import { describe, expect, it } from 'vitest';

import { conditionsHold, countingOutcome } from './evaluate';
import { defaultCounting, MIB } from './model';
import { runPipelineTest } from '../editor/test/run';
import { defaultPipelineGraph } from '../PipelineGraph/model';

describe('counting steps in a test', () => {
  it('pick events the way the backend reads its tests', () => {
    const errors = [
      { field: 'level', test: 'equals' as const, value: 'error' },
      { field: 'status', test: 'gte' as const, value: '500' },
    ];

    expect(conditionsHold(errors, { level: 'error', status: 503 })).toBe(true);
    expect(conditionsHold(errors, { level: 'error', status: '500' })).toBe(true);
    expect(conditionsHold(errors, { level: 'error', status: 404 })).toBe(false);
    expect(conditionsHold([{ field: 'service.name', test: 'equals', value: 'cart' }], { service: { name: 'cart' } })).toBe(true);
    expect(conditionsHold([{ field: 'env', test: 'not_equals', value: 'prod' }], {})).toBe(true);
    expect(conditionsHold([{ field: 'ok', test: 'equals', value: 'true' }], { ok: true })).toBe(true);
    expect(conditionsHold([], {})).toBe(true);
  });

  it('say what a metric would count', () => {
    const parts = defaultCounting('generate_metric', 'app');
    const metric = { ...parts.metric!, name: 'latency_ms', type: 'histogram' as const, field: 'duration_ms', groupBy: ['service', 'region'] };

    expect(countingOutcome('generate_metric', { conditions: [], metric }, { service: 'cart', duration_ms: '42' })).toEqual({
      kind: 'metric',
      series: 'latency_ms{service="cart"}',
      type: 'histogram',
      field: 'duration_ms',
      value: 42,
    });
    expect(countingOutcome('generate_metric', { conditions: [], metric }, { duration_ms: 'slow' })).toMatchObject({ value: null });
    expect(
      countingOutcome(
        'generate_metric',
        { conditions: [{ field: 'level', test: 'equals', value: 'error' }], metric },
        { level: 'info' },
      ),
    ).toEqual({ kind: 'not_picked' });
  });

  it('name the bucket of a quota and its own limit', () => {
    const outcome = countingOutcome(
      'quota',
      {
        conditions: [],
        quota: {
          unit: 'bytes',
          limit: '10',
          partitionBy: ['service'],
          overrides: [{ values: { service: 'checkout' }, limit: '2' }],
          action: 'route',
          target: 'cold',
        },
      },
      { service: 'checkout' },
    );

    expect(outcome).toEqual({
      kind: 'limit',
      step: 'quota',
      bucket: [['service', 'checkout']],
      limit: (2 * MIB) / MIB,
      per: 'bytes',
      action: 'route',
      target: 'cold',
    });
  });

  it('let the event on, unchanged, in a pipeline run', async () => {
    const graph = {
      ...defaultPipelineGraph('logs'),
      mode: 'realtime' as const,
      sources: ['app'],
      sinks: ['app'],
      transforms: [
        { name: 'burst', kind: 'throttle' as const, script: '', ...defaultCounting('throttle') },
        { name: 'errors', kind: 'generate_metric' as const, script: '', ...defaultCounting('generate_metric', 'app') },
      ],
    };
    const event = { level: 'error', host: 'web-1' };

    const outcome = await runPipelineTest(graph, event, () => Promise.reject(new Error('no VRL here')));

    expect(outcome.steps.map((step) => step.status)).toEqual(['passed', 'passed']);
    expect(outcome.steps[0]?.counting).toMatchObject({ kind: 'limit', step: 'throttle', action: 'drop' });
    expect(outcome.steps[1]?.counting).toMatchObject({ kind: 'metric', value: 1 });
    expect(outcome.output).toEqual(event);
    expect(outcome.destinations).toEqual(['app']);
  });
});
