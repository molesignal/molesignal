import { describe, expect, it } from 'vitest';

import {
  defaultPipelineGraph,
  pipelineInputFromGraph,
  pipelineGraphFromPipeline,
  isValidGraphConnection,
  pipelineGraphStats,
  pipelineStreamOptions,
  validateGraph,
} from '../PipelineGraph';

const conn = (source: string, target: string) => ({
  source,
  target,
  sourceHandle: null,
  targetHandle: null,
});

describe('isValidGraphConnection', () => {
  it('allows forward source → transform → sink', () => {
    expect(isValidGraphConnection(conn('source-0', 'transform-0'))).toBe(true);
    expect(isValidGraphConnection(conn('transform-0', 'transform-1'))).toBe(true);
    expect(isValidGraphConnection(conn('transform-0', 'sink-0'))).toBe(true);
    expect(isValidGraphConnection(conn('source-0', 'sink-0'))).toBe(true);
  });

  it('rejects into a source, out of a sink, self-loops, and backward transform edges', () => {
    expect(isValidGraphConnection(conn('transform-0', 'source-0'))).toBe(false);
    expect(isValidGraphConnection(conn('sink-0', 'transform-0'))).toBe(false);
    expect(isValidGraphConnection(conn('transform-0', 'transform-0'))).toBe(false);
    expect(isValidGraphConnection(conn('transform-1', 'transform-0'))).toBe(false);
  });
});

describe('validateGraph', () => {
  it('passes a default graph with no issues', () => {
    expect(validateGraph(defaultPipelineGraph('logs'))).toEqual([]);
  });

  it('flags a missing source and a transform with no script', () => {
    const base = defaultPipelineGraph('logs');
    const codes = validateGraph({
      ...base,
      sources: [],
      transforms: [{ name: 'x', script: '   ' }],
    }).map((issue) => issue.code);
    expect(codes).toContain('no_source');
    expect(codes).toContain('transform_script_missing');
  });

  it('warns when a connector sink references an unknown connector', () => {
    const base = defaultPipelineGraph('logs');
    const codes = validateGraph({ ...base, sinks: ['connector:ghost'] }, ['real']).map(
      (issue) => issue.code,
    );
    expect(codes).toContain('connector_missing');
  });

  it('warns when a source and sink share a name (loop)', () => {
    const base = defaultPipelineGraph('logs');
    const codes = validateGraph({ ...base, sources: ['dup'], sinks: ['dup'] }).map(
      (issue) => issue.code,
    );
    expect(codes).toContain('source_is_sink');
  });
});

describe('pipelineGraphStats', () => {
  it('counts workbench nodes and generated forward connections', () => {
    const graph = {
      ...defaultPipelineGraph('logs'),
      sources: ['app_logs', 'audit_logs'],
      transforms: [
        { name: 'normalize', script: '.level = downcase(.level)' },
        { name: 'enrich', script: '.environment = "prod"' },
      ],
      sinks: ['logs_enriched', 'connector:archive'],
    };

    expect(pipelineGraphStats(graph)).toEqual({ nodes: 6, edges: 5 });
  });
});

describe('pipelineStreamOptions', () => {
  const stream = (name: string, streamType: 'logs' | 'metrics' | 'traces') => ({
    id: name,
    label: name,
    name,
    stream_type: streamType,
    type: streamType,
    schema: { fields: [] },
    retention: null,
    effective_retention: { days: 30 },
    settings: {
      description: null,
      index_rules: [],
      retention_filter: null,
      keep_conditions: [],
      max_query_range_hours: null,
      flatten_level: null,
      use_stream_stats_for_partitioning: false,
      store_original_data: false,
      enable_distinct_values: true,
      queryable: true,
    },
    created_at_micros: 0,
    updated_at_micros: 0,
  });

  it('only exposes streams matching the pipeline signal type', () => {
    const options = pipelineStreamOptions(
      [
        stream('z_logs', 'logs'),
        stream('metrics_total', 'metrics'),
        stream('a_logs', 'logs'),
      ],
      'logs',
    );

    expect(options).toEqual([
      { value: 'a_logs', label: 'a_logs' },
      { value: 'z_logs', label: 'z_logs' },
    ]);
  });

  it('preserves an existing stream that is not currently in the catalog', () => {
    const options = pipelineStreamOptions(
      [stream('app_logs', 'logs')],
      'logs',
      'legacy_logs',
      (name) => `${name} (current value)`,
    );

    expect(options[0]).toEqual({
      value: 'legacy_logs',
      label: 'legacy_logs (current value)',
    });
  });

  it('does not expose connector values as data stream options', () => {
    expect(
      pipelineStreamOptions([], 'logs', 'connector:archive'),
    ).toEqual([]);
  });
});

describe('realtime routing configuration', () => {
  it('allows routing back to the source while rejecting multiple output nodes', () => {
    const graph = { ...defaultPipelineGraph('logs'), mode: 'realtime' as const, sources: ['default'], sinks: ['default'], routing: { kind: 'field' as const, field: 'appname', prefix: 'app_', fallback: 'default', retain_source: true } };
    expect(validateGraph(graph)).toEqual([]);
    expect(validateGraph({ ...graph, sinks: ['a', 'b'] }).map((issue) => issue.code)).toContain('realtime_nodes');
    expect(validateGraph({ ...graph, signalType: 'metrics' }).map((issue) => issue.code)).toContain('realtime_logs');
  });
});

it('roundtrips realtime output settings and removes the schedule', () => {
  const graph = { ...defaultPipelineGraph('logs'), mode: 'realtime' as const, sources: ['default'], sinks: ['default'], routing: { kind: 'field' as const, field: 'appname', prefix: 'app_', fallback: 'default', retain_source: true } };
  const payload = pipelineInputFromGraph({ name: 'route', graph, cron: 'every:5m' });
  expect(payload.cron).toBe('');
  const restored = pipelineGraphFromPipeline({ ...payload, id: 'one' });
  expect(restored.mode).toBe('realtime');
  expect(restored.routing).toBeUndefined();
  expect(restored.retainSource).toBe(true);
  expect(restored.transforms.at(-1)).toMatchObject({ kind: 'builtin', operation: 'route', routing: { kind: 'field', field: 'appname' } });
  expect((payload.function_steps as Record<string, unknown>).routing).toBeUndefined();
  expect(restored.sources).toEqual(['default']);
});

it('roundtrips interleaved built-in and VRL steps without generating a script for routing', () => {
  const graph = { ...defaultPipelineGraph('logs'), transforms: [
    { name: 'parse', kind: 'vrl' as const, script: '.appname = "orders"' },
    { name: 'route', kind: 'builtin' as const, operation: 'route' as const, script: '', target: 'orders', routing: { kind: 'fixed' as const, field: 'appname', prefix: '', fallback: 'default', retain_source: false } },
    { name: 'enrich', kind: 'vrl' as const, script: '.processed = true' },
  ] };
  expect(validateGraph(graph)).toEqual([]);
  const payload = pipelineInputFromGraph({ name: 'ordered', graph, cron: 'every:5m' });
  const steps = (payload.function_steps as { steps: Record<string, unknown>[] }).steps;
  expect(steps[1]).toMatchObject({ kind: 'builtin', operation: 'route', target: 'orders' });
  expect(steps[1]).not.toHaveProperty('script');
  expect(pipelineGraphFromPipeline({ ...payload, id: 'one' }).transforms.map((step) => step.kind ?? 'vrl')).toEqual(['vrl', 'builtin', 'vrl']);
});
