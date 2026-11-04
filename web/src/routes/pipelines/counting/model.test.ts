import { describe, expect, it } from 'vitest';

import {
  countingInput,
  countingIssue,
  countingKinds,
  countingOf,
  countingProblem,
  defaultCounting,
  metricQuery,
  MIB,
  type CountingParts,
} from './model';
import { pipelineGraphFromPipeline, pipelineInputFromGraph } from '../PipelineGraph/model';
import { validateGraph } from '../PipelineGraph/validation';
import { newStep } from '../editor/steps';

const metric = (patch: Partial<NonNullable<CountingParts['metric']>> = {}): CountingParts => ({
  conditions: [],
  metric: { ...defaultCounting('generate_metric', 'app').metric!, name: 'app_errors', ...patch },
});

describe('counting steps', () => {
  it('are offered where their data allows', () => {
    expect(countingKinds('realtime', 'logs', 'app')).toEqual(['generate_metric', 'quota', 'throttle']);
    expect(countingKinds('realtime', 'traces', 'app')).toEqual(['generate_metric']);
    expect(countingKinds('realtime', 'metrics', 'app')).toEqual([]);
    expect(countingKinds('realtime', 'logs', 'rum_views')).toEqual(['generate_metric']);
    expect(countingKinds('realtime', 'logs', 'rum_errors')).toEqual(['generate_metric', 'quota', 'throttle']);
    expect(countingKinds('scheduled', 'logs', 'app')).toEqual([]);
  });

  it('start with a metric written into a metrics stream named like the source', () => {
    const step = newStep('generate_metric', [], 'realtime', ' nginx ');

    expect(step).toMatchObject({ name: 'metric', kind: 'generate_metric', metric: { stream: 'nginx', type: 'count' } });
    expect(newStep('quota', ['quota'], 'realtime').name).toBe('quota-2');
    expect(newStep('throttle', [], 'realtime').throttle).toMatchObject({ windowSecs: '60', action: 'drop' });
  });

  it('save what the backend takes and read back the same', () => {
    const pipeline = {
      id: 'p',
      name: 'nginx',
      sourceStream: 'nginx',
      targetStream: 'nginx',
      cron: '',
      lookbackSecs: 300,
      enabled: true,
      functionSteps: {
        mode: 'realtime',
        signalType: 'logs',
        steps: [
          {
            transformName: 'latency',
            kind: 'generate_metric',
            conditions: [{ field: 'status', test: 'gte', value: '500' }],
            metric: {
              name: 'latency_ms',
              type: 'histogram',
              field: 'duration_ms',
              groupBy: ['service'],
              buckets: [10, 100],
              stream: 'nginx',
              intervalSecs: 10,
            },
          },
          {
            transformName: 'daily',
            kind: 'quota',
            conditions: [],
            quota: {
              unit: 'bytes',
              limit: 10 * MIB,
              partitionBy: ['service'],
              overrides: [{ values: { service: 'checkout' }, limit: MIB / 2 }],
              action: 'route',
              target: 'nginx_cold',
            },
          },
          {
            transformName: 'burst',
            kind: 'throttle',
            conditions: [{ field: 'level', test: 'exists' }],
            throttle: { limit: 100, windowSecs: 60, groupBy: ['host'], action: 'drop' },
          },
        ],
      },
    };

    const graph = pipelineGraphFromPipeline(pipeline as never);
    const [latency, daily, burst] = graph.transforms;

    expect(latency?.metric).toMatchObject({ type: 'histogram', buckets: ['10', '100'], intervalSecs: 10 });
    expect(daily?.quota).toMatchObject({ unit: 'bytes', limit: '10', overrides: [{ limit: '0.5' }] });
    expect(burst?.throttle).toMatchObject({ limit: '100', windowSecs: '60', groupBy: ['host'] });
    expect(validateGraph(graph)).toEqual([]);

    const saved = pipelineInputFromGraph({ name: 'nginx', graph, cron: '' });
    expect((saved.functionSteps as { steps: unknown[] }).steps).toEqual(pipeline.functionSteps.steps);
  });

  it('say what they still lack or get wrong', () => {
    expect(countingProblem('generate_metric', metric({ name: '' }))).toBe('processor_field_missing');
    expect(countingProblem('generate_metric', metric({ name: '5xx' }))).toBe('metric_name_invalid');
    expect(countingProblem('generate_metric', metric({ type: 'gauge' }))).toBe('processor_field_missing');
    expect(countingProblem('generate_metric', metric({ groupBy: ['le'] }))).toBe('metric_label_reserved');
    expect(
      countingProblem('generate_metric', metric({ type: 'histogram', field: 'd', buckets: ['50', '10'] })),
    ).toBe('metric_buckets_invalid');
    expect(countingProblem('generate_metric', metric())).toBeNull();

    const quota = defaultCounting('quota');
    expect(countingProblem('quota', quota)).toBe('limit_invalid');
    expect(countingProblem('quota', { ...quota, quota: { ...quota.quota!, limit: '1000', action: 'route' } })).toBe(
      'limit_target_missing',
    );
    expect(
      countingProblem('quota', {
        ...quota,
        quota: { ...quota.quota!, limit: '1000', overrides: [{ values: {}, limit: '5' }] },
      }),
    ).toBe('limit_overrides_invalid');

    const throttle = defaultCounting('throttle');
    expect(countingProblem('throttle', { ...throttle, throttle: { ...throttle.throttle!, limit: '10' } })).toBeNull();
    expect(
      countingProblem('throttle', {
        conditions: [{ field: 'status', test: 'gt', value: 'high' }],
        throttle: { ...throttle.throttle!, limit: '10' },
      }),
    ).toBe('counting_condition_invalid');
  });

  it('send what goes past a limit only to a stream they can write, never back into the source', () => {
    const routed = (target: string): CountingParts => ({
      conditions: [],
      quota: { unit: 'events', limit: '10', partitionBy: [], overrides: [], action: 'route', target },
    });
    const all = countingKinds('realtime', 'logs', 'app');

    expect(countingIssue('quota', routed('app'), all, 'app', 'realtime')?.code).toBe('limit_target_source');
    expect(countingIssue('quota', routed('rum_views'), all, 'app', 'realtime')?.code).toBe('route_to_rum');
    expect(countingIssue('quota', routed('cold'), all, 'app', 'realtime')).toBeNull();
    expect(countingIssue('quota', routed('cold'), all, 'rum_errors', 'realtime')?.code).toBe('in_place_route');
    expect(countingIssue('quota', routed('cold'), [], 'app', 'scheduled')?.code).toBe('counting_realtime_only');
    expect(countingIssue('quota', routed('cold'), ['generate_metric'], 'app', 'realtime')?.code).toBe(
      'counting_not_here_limit',
    );
  });

  it('name the query that reads the metric', () => {
    expect(metricQuery(metric({ groupBy: ['service.name'] }).metric!)).toBe(
      'sum by ("service.name") (increase(app_errors[5m]))',
    );
    expect(metricQuery(metric({ type: 'histogram', name: 'latency', groupBy: [] }).metric!)).toBe(
      'histogram_quantile(0.99, sum by (le) (rate(latency_bucket[5m])))',
    );
  });

  it('read a saved step that says little as the defaults', () => {
    expect(countingOf('throttle', {})).toMatchObject({ throttle: { limit: '', windowSecs: '60', action: 'drop' } });
    expect(countingInput('quota', defaultCounting('quota'))).toMatchObject({ quota: { limit: 0, action: 'drop' } });
  });
});
