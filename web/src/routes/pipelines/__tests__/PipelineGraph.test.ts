import { describe, expect, it } from 'vitest';

import {
  blocksSave,
  defaultPipelineGraph,
  MAX_STREAMS,
  pipelineInputFromGraph,
  pipelineGraphFromPipeline,
  pipelineGraphStats,
  pipelineStreamOptions,
  pipelineStreams,
  stepType,
  validateGraph,
  type PipelineGraphModel,
  type TransformStep,
} from '../PipelineGraph';
import { compileProcessor, type ProcessorConfig } from '../processors/catalog';

/** A pipeline with everything chosen: reads one stream, writes another, no steps. */
function graph(overrides: Partial<PipelineGraphModel> = {}): PipelineGraphModel {
  return { ...defaultPipelineGraph('logs'), sources: ['app_logs'], sinks: ['clean_logs'], ...overrides };
}

const route = (overrides: Partial<TransformStep> = {}, routing: Partial<TransformStep['routing']> = {}): TransformStep => ({
  name: 'route',
  kind: 'builtin',
  operation: 'route',
  script: '',
  target: 'orders',
  ...overrides,
  routing: { kind: 'fixed', field: '', prefix: '', fallback: 'default', retain_source: false, ...routing },
});

const found = (model: PipelineGraphModel, connectors?: string[]) =>
  validateGraph(model, connectors).map((issue) => [issue.level, issue.code, issue.node]);

describe('validateGraph', () => {
  it('asks a new pipeline for the two things it has to be told, and calls neither a mistake', () => {
    const issues = validateGraph(defaultPipelineGraph('logs'));

    expect(issues).toEqual([
      { level: 'error', code: 'source_missing', node: 'source-0', pending: true },
      { level: 'error', code: 'sink_missing', node: 'sink-0', pending: true },
    ]);
    expect(blocksSave(issues)).toBe(true);
  });

  it('passes a pipeline that reads one stream and writes another, with or without steps', () => {
    expect(validateGraph(graph())).toEqual([]);
    expect(validateGraph(graph({ transforms: [{ name: 'keep', script: '. = .' }] }))).toEqual([]);
    expect(blocksSave([])).toBe(false);
  });

  it('holds names to what a stream may be called', () => {
    expect(validateGraph(graph({ sinks: ['clean logs'] }))).toEqual([
      { level: 'error', code: 'stream_name_invalid', params: { name: 'clean logs' }, node: 'sink-0' },
    ]);
    // The product's own stream is nobody's to read or write.
    expect(found(graph({ mode: 'realtime', sources: ['_molesignal'] }))).toEqual([
      ['error', 'stream_name_invalid', 'source-0'],
    ]);
    expect(found(graph({ sinks: ['..'] }))).toEqual([['error', 'stream_name_invalid', 'sink-0']]);
  });

  it('will not write a scheduled pipeline into any stream it reads', () => {
    expect(validateGraph(graph({ sinks: ['app_logs'] }))).toEqual([
      { level: 'error', code: 'source_is_sink', params: { name: 'app_logs' }, node: 'sink-0' },
    ]);
    expect(
      found(graph({ sources: ['app_logs', 'audit_logs'], sinks: ['clean_logs', 'audit_logs'] })),
    ).toEqual([['error', 'source_is_sink', 'sink-1']]);
    // A realtime pipeline may: the processed event takes the place of the one that arrived.
    expect(validateGraph(graph({ mode: 'realtime', sinks: ['app_logs'] }))).toEqual([]);
  });

  it('reads every source and writes every stream, and says when a name is there twice', () => {
    const issues = validateGraph(
      graph({
        sources: ['app_logs', 'audit_logs', 'app_logs'],
        sinks: ['clean_logs', 'connector:c1', 'archive_logs', 'clean_logs'],
      }),
      ['c1'],
    );

    expect(issues).toEqual([
      { level: 'warning', code: 'source_duplicate', params: { name: 'app_logs' }, node: 'source-2' },
      { level: 'warning', code: 'sink_duplicate', params: { name: 'clean_logs' }, node: 'sink-3' },
    ]);
    expect(blocksSave(issues)).toBe(false);
    // Each source still to be chosen is work to do.
    expect(found(graph({ sources: ['app_logs', ''], sinks: ['clean_logs', ''] }))).toEqual([
      ['error', 'source_missing', 'source-1'],
      ['error', 'sink_missing', 'sink-1'],
    ]);
  });

  it(`holds a pipeline to ${MAX_STREAMS} streams on either side`, () => {
    const names = (prefix: string, count: number) =>
      Array.from({ length: count }, (_, index) => `${prefix}_${index}`);

    expect(validateGraph(graph({ sources: names('in', MAX_STREAMS), sinks: names('out', MAX_STREAMS) }))).toEqual(
      [],
    );
    expect(found(graph({ sources: names('in', MAX_STREAMS + 1) }))).toEqual([
      ['error', 'too_many_sources', undefined],
    ]);
    // Connectors do not count.
    expect(
      found(graph({ sinks: [...names('out', MAX_STREAMS), 'connector:c1'] })),
    ).toEqual([]);
    expect(found(graph({ sinks: names('out', MAX_STREAMS + 1) }))).toEqual([
      ['error', 'too_many_sinks', undefined],
    ]);
  });

  it('wants a dead letter stream the pipeline neither reads nor writes', () => {
    expect(validateGraph(graph({ deadLetterStream: 'failed_events' }))).toEqual([]);
    expect(validateGraph(graph({ deadLetterStream: 'failed events' }))).toEqual([
      { level: 'error', code: 'dead_letter_invalid', params: { name: 'failed events' } },
    ]);
    for (const taken of ['app_logs', 'clean_logs']) {
      expect(found(graph({ deadLetterStream: taken }))).toEqual([['error', 'dead_letter_conflict', undefined]]);
    }
    // A realtime pipeline keeps no dead letters, so there is nothing to check.
    expect(validateGraph(graph({ mode: 'realtime', deadLetterStream: 'app_logs' }))).toEqual([]);
  });

  it('wants a connector chosen, once, and one the workspace has', () => {
    expect(
      validateGraph(graph({ sinks: ['clean_logs', 'connector:', 'connector:c1', 'connector:c1', 'connector:ghost'] }), [
        'c1',
      ]),
    ).toEqual([
      { level: 'error', code: 'connector_unset', node: 'sink-1', pending: true },
      { level: 'warning', code: 'connector_duplicate', node: 'sink-3' },
      { level: 'warning', code: 'connector_missing', params: { id: 'ghost' }, node: 'sink-4' },
    ]);
    // Before the workspace's connectors are known, none is called missing.
    expect(validateGraph(graph({ sinks: ['clean_logs', 'connector:ghost'] }))).toEqual([]);
  });

  it('needs a stream to write to, which connectors alone are not', () => {
    expect(found(graph({ sinks: ['connector:c1'] }))).toEqual([['error', 'no_sink', undefined]]);
    // Unless a routing step says where each event goes.
    expect(validateGraph(graph({ sinks: ['connector:c1'], transforms: [route()] }))).toEqual([]);
    expect(found(graph({ sources: [] }))).toEqual([['error', 'no_source', undefined]]);
  });

  it('keeps realtime to logs, one stream in and one out', () => {
    const realtime = graph({ mode: 'realtime' });

    expect(validateGraph(realtime)).toEqual([]);
    expect(found({ ...realtime, signalType: 'metrics' })).toEqual([['error', 'realtime_logs', undefined]]);
    expect(found({ ...realtime, sinks: ['clean_logs', 'connector:c1'] })).toEqual([
      ['error', 'realtime_nodes', undefined],
    ]);
    expect(found({ ...realtime, sources: ['app_logs', 'audit_logs'] })).toContainEqual([
      'error',
      'realtime_nodes',
      undefined,
    ]);
  });

  it('leaves the first stream to a routing step when there is one, and checks the others', () => {
    expect(validateGraph(graph({ sinks: [''], transforms: [route()] }))).toEqual([]);
    expect(found(graph({ sinks: ['', ''], transforms: [route()] }))).toEqual([['error', 'sink_missing', 'sink-1']]);
    expect(found(graph({ sinks: ['', 'app_logs'], transforms: [route()] }))).toEqual([
      ['error', 'source_is_sink', 'sink-1'],
    ]);
  });

  it('checks a routing step the way the backend will', () => {
    const one = (step: TransformStep, overrides: Partial<PipelineGraphModel> = {}) =>
      validateGraph(graph({ transforms: [step], ...overrides }));

    expect(one(route({ target: ' ' }))).toEqual([
      { level: 'error', code: 'route_target_missing', params: { name: 'route' }, node: 'transform-0', pending: true },
    ]);
    expect(one(route({}, { kind: 'field', field: '', fallback: 'default' }))).toEqual([
      { level: 'error', code: 'route_field_missing', params: { name: 'route' }, node: 'transform-0', pending: true },
    ]);
    expect(one(route({}, { kind: 'field', field: 'appname', fallback: '' }))[0]?.code).toBe('route_field_missing');
    expect(one(route({ target: 'my orders' }))).toEqual([
      {
        level: 'error',
        code: 'route_stream_invalid',
        params: { name: 'route', stream: 'my orders' },
        node: 'transform-0',
      },
    ]);
    expect(one(route({}, { kind: 'field', field: 'appname', prefix: 'app/' }))[0]?.code).toBe('route_prefix_invalid');
    expect(one(route({}, { kind: 'field', field: 'appname', prefix: 'app_' }))).toEqual([]);
  });

  it('will not route a scheduled pipeline into a stream it reads', () => {
    const back = graph({ transforms: [route({ target: 'app_logs' })] });

    expect(validateGraph(back)).toEqual([
      {
        level: 'error',
        code: 'route_to_source',
        params: { name: 'route', stream: 'app_logs' },
        node: 'transform-0',
      },
    ]);
    // Falling back to it is the same thing.
    expect(
      found(graph({ transforms: [route({}, { kind: 'field', field: 'appname', fallback: 'app_logs' })] })),
    ).toEqual([['error', 'route_to_source', 'transform-0']]);
    expect(validateGraph({ ...back, mode: 'realtime' })).toEqual([]);
    // Any of the streams it reads.
    expect(
      found(graph({ sources: ['app_logs', 'audit_logs'], transforms: [route({ target: 'audit_logs' })] })),
    ).toEqual([['error', 'route_to_source', 'transform-0']]);
  });

  it('holds an unfinished step against that step, as work to do', () => {
    const rename: ProcessorConfig = { op: 'rename', from: 'msg', to: '' };
    const price: ProcessorConfig = { op: 'set', field: 'price', valueType: 'number', value: 'ten' };
    const issues = validateGraph(
      graph({
        transforms: [
          { name: 'rename', script: compileProcessor(rename), processor: rename },
          { name: 'price', script: compileProcessor(price), processor: price },
          { name: '', script: '  ' },
        ],
      }),
    );

    expect(issues).toEqual([
      { level: 'error', code: 'processor_field_missing', params: { name: 'rename' }, node: 'transform-0', pending: true },
      // Written, and wrong: not something left to fill in.
      { level: 'error', code: 'processor_number_invalid', params: { name: 'price' }, node: 'transform-1' },
      { level: 'warning', code: 'transform_name_missing', params: { index: 3 }, node: 'transform-2' },
      { level: 'error', code: 'transform_script_missing', params: { name: '3' }, node: 'transform-2', pending: true },
    ]);
  });

  it('holds a sample to a share of events that is one', () => {
    const sample: ProcessorConfig = { op: 'sample', percent: '150', by: '' };

    expect(
      validateGraph(graph({ transforms: [{ name: 'sample', script: compileProcessor(sample), processor: sample }] })),
    ).toEqual([
      { level: 'error', code: 'processor_percent_invalid', params: { name: 'sample' }, node: 'transform-0' },
    ]);
  });
});

describe('a pipeline as the editor opens it', () => {
  it('starts with a source and a destination to choose, and no steps', () => {
    expect(pipelineGraphFromPipeline(null)).toEqual({
      mode: 'scheduled',
      retainSource: false,
      signalType: 'logs',
      sources: [''],
      sinks: [''],
      transforms: [],
      // What the backend does when a pipeline says nothing of it.
      retry: { runRetries: 3, deliveryRetries: 3 },
      deadLetterStream: '',
      dedupeField: '',
      delaySecs: 60,
    });
    expect(defaultPipelineGraph('metrics').signalType).toBe('metrics');
  });

  it('leads with the stream that is read and the stream that is written, whatever order the lists were saved in', () => {
    const opened = pipelineGraphFromPipeline({
      id: 'p',
      name: 'p',
      source_stream: 'app_logs',
      target_stream: 'clean_logs',
      function_steps: {
        sources: ['audit_logs', 'app_logs'],
        sinks: ['archive_logs', 'clean_logs'],
        sink_connectors: ['c1', 'c1'],
        steps: [],
      },
    });

    expect(opened.sources).toEqual(['app_logs', 'audit_logs']);
    expect(opened.sinks).toEqual(['clean_logs', 'archive_logs', 'connector:c1']);
    expect(opened.transforms).toEqual([]);
  });

  it('shows the steps that are saved and makes none up', () => {
    const open = (function_steps: unknown) =>
      pipelineGraphFromPipeline({ id: 'p', name: 'p', source_stream: 'a', target_stream: 'b', function_steps })
        .transforms;

    expect(open(undefined)).toEqual([]);
    expect(open({ steps: [] })).toEqual([]);
    expect(open({ steps: [{ transform_name: 'keep', script: '. = .' }] })).toEqual([{ name: 'keep', script: '. = .' }]);
    // Shapes older editors saved.
    expect(open([{ function_name: 'old', script: '.a = 1' }])).toEqual([{ name: 'old', script: '.a = 1' }]);
    expect(open({ transform_name: 'single', script: '.b = 2' })).toEqual([{ name: 'single', script: '.b = 2' }]);
    expect(open('.c = 3')).toEqual([{ name: '', script: '.c = 3' }]);
  });
});

describe('a pipeline as it is saved', () => {
  it('sends what is in the flow', () => {
    const payload = pipelineInputFromGraph({
      name: 'tidy',
      graph: graph({
        sinks: ['clean_logs', 'connector:c1', 'connector:'],
        transforms: [
          { name: ' parse ', script: '. = parse_json!(.message)' },
          { name: '', script: '.seen = true' },
        ],
      }),
      cron: 'every:5m',
      lookbackSecs: 300,
      enabled: true,
    });

    expect(payload).toEqual({
      name: 'tidy',
      source_stream: 'app_logs',
      target_stream: 'clean_logs',
      cron: 'every:5m',
      lookback_secs: 300,
      enabled: true,
      function_steps: {
        mode: 'scheduled',
        signal_type: 'logs',
        sources: ['app_logs'],
        sinks: ['clean_logs'],
        // A connector nobody chose is not one.
        sink_connectors: ['c1'],
        delay_secs: 60,
        retry: { run_retries: 3, delivery_retries: 3 },
        steps: [
          { transform_name: 'parse', kind: 'vrl', script: '. = parse_json!(.message)' },
          // A step nobody named goes by its place.
          { transform_name: 'step-2', kind: 'vrl', script: '.seen = true' },
        ],
      },
    });
  });

  it('saves no steps as no steps', () => {
    const payload = pipelineInputFromGraph({ name: 'copy', graph: graph(), cron: 'every:1h' });

    expect((payload.function_steps as { steps: unknown[] }).steps).toEqual([]);
  });

  it('makes nothing up for what was left empty', () => {
    const payload = pipelineInputFromGraph({ name: 'new', graph: defaultPipelineGraph('logs'), cron: 'every:5m' });

    expect(payload.source_stream).toBe('');
    expect(payload.target_stream).toBe('');
    expect(payload.function_steps).toMatchObject({ sources: [], sinks: [], steps: [] });
  });

  it('names the stream a routing step falls back to as the one that is written', () => {
    const payload = pipelineInputFromGraph({
      name: 'routed',
      graph: graph({
        sinks: ['clean_logs', 'connector:c1'],
        transforms: [route({}, { kind: 'field', field: 'appname', prefix: 'app_', fallback: 'misc' })],
      }),
      cron: 'every:5m',
    });

    expect(payload.target_stream).toBe('misc');
    expect(payload.function_steps).toMatchObject({ sinks: ['misc'], sink_connectors: ['c1'] });
  });

  it('keeps every other stream beside a routing step, to take every event', () => {
    const payload = pipelineInputFromGraph({
      name: 'routed',
      graph: graph({
        sinks: ['', 'archive_logs', 'connector:c1', 'audit_logs'],
        transforms: [route({ target: 'orders' })],
      }),
      cron: 'every:5m',
    });

    expect(payload.target_stream).toBe('orders');
    expect(payload.function_steps).toMatchObject({ sinks: ['orders', 'archive_logs', 'audit_logs'] });
  });

  it('sends every source and every stream, the first of each as the pipeline’s own', () => {
    const payload = pipelineInputFromGraph({
      name: 'fan',
      graph: graph({ sources: ['app_logs', ' audit_logs '], sinks: ['clean_logs', 'archive_logs'] }),
      cron: 'every:5m',
    });

    expect(payload).toMatchObject({ source_stream: 'app_logs', target_stream: 'clean_logs' });
    expect(payload.function_steps).toMatchObject({
      sources: ['app_logs', 'audit_logs'],
      sinks: ['clean_logs', 'archive_logs'],
    });
  });

  it('saves how a scheduled pipeline reads and fails, and opens it again', () => {
    const model = graph({
      retry: { runRetries: 0, deliveryRetries: 5 },
      deadLetterStream: ' failed_events ',
      dedupeField: ' trace_id ',
      delaySecs: 300,
    });
    const payload = pipelineInputFromGraph({ name: 'careful', graph: model, cron: 'every:5m' });

    expect(payload.function_steps).toMatchObject({
      delay_secs: 300,
      retry: { run_retries: 0, delivery_retries: 5 },
      dead_letter_stream: 'failed_events',
      dedupe_field: 'trace_id',
    });
    expect(pipelineGraphFromPipeline({ ...payload, id: 'one' })).toMatchObject({
      retry: { runRetries: 0, deliveryRetries: 5 },
      deadLetterStream: 'failed_events',
      dedupeField: 'trace_id',
      delaySecs: 300,
    });
    // Blank is none: nothing is saved for it.
    const plain = pipelineInputFromGraph({ name: 'plain', graph: graph(), cron: 'every:5m' });
    expect(plain.function_steps).not.toHaveProperty('dead_letter_stream');
    expect(plain.function_steps).not.toHaveProperty('dedupe_field');
    // A realtime pipeline neither waits nor retries.
    const realtime = pipelineInputFromGraph({ name: 'live', graph: { ...model, mode: 'realtime' }, cron: '' });
    expect(realtime.function_steps).not.toHaveProperty('retry');
    expect(realtime.function_steps).not.toHaveProperty('delay_secs');
    expect(realtime.function_steps).not.toHaveProperty('dead_letter_stream');
  });

  it('opens a saved value out of range as the default', () => {
    const opened = pipelineGraphFromPipeline({
      id: 'p',
      name: 'p',
      source_stream: 'a',
      target_stream: 'b',
      function_steps: { retry: { run_retries: 99, delivery_retries: 'x' }, delay_secs: -5 },
    });

    expect(opened).toMatchObject({ retry: { runRetries: 3, deliveryRetries: 3 }, delaySecs: 60 });
  });
});

describe('processor steps', () => {
  const rename: ProcessorConfig = { op: 'rename', from: 'msg', to: 'message' };
  const model = graph({
    transforms: [{ name: 'rename', script: compileProcessor(rename), processor: rename }],
  });

  it('are saved as the VRL they stand for, with the form beside it', () => {
    const payload = pipelineInputFromGraph({ name: 'p', graph: model, cron: 'every:5m' });

    expect((payload.function_steps as { steps: unknown[] }).steps).toEqual([
      {
        transform_name: 'rename',
        kind: 'vrl',
        script: 'if exists(.msg) {\n  .message = del(.msg)\n}',
        processor: rename,
      },
    ]);
  });

  it('open as a form again', () => {
    const payload = pipelineInputFromGraph({ name: 'p', graph: model, cron: 'every:5m' });
    const restored = pipelineGraphFromPipeline({ ...payload, id: 'one' });

    expect(restored.transforms).toEqual(model.transforms);
    expect(stepType(restored.transforms[0]!)).toBe('rename');
  });

  it('open as VRL once the script is no longer the one the form writes', () => {
    const payload = pipelineInputFromGraph({ name: 'p', graph: model, cron: 'every:5m' });
    const steps = payload.function_steps as { steps: Array<Record<string, unknown>> };
    steps.steps[0]!.script = '.message = del(.msg) # edited by hand';

    const restored = pipelineGraphFromPipeline({ ...payload, id: 'one' });

    expect(restored.transforms[0]).toEqual({
      name: 'rename',
      script: '.message = del(.msg) # edited by hand',
    });
    expect(stepType(restored.transforms[0]!)).toBe('vrl');
  });

  it('do not save a form whose script was replaced', () => {
    const edited = { ...model, transforms: [{ ...model.transforms[0]!, script: '. = .' }] };
    const payload = pipelineInputFromGraph({ name: 'p', graph: edited, cron: 'every:5m' });

    expect((payload.function_steps as { steps: unknown[] }).steps).toEqual([
      { transform_name: 'rename', kind: 'vrl', script: '. = .' },
    ]);
  });
});

describe('pipelineGraphStats', () => {
  it('counts every source, step and destination, and the links between them', () => {
    const model = graph({
      sources: ['app_logs', 'audit_logs'],
      transforms: [
        { name: 'normalize', script: '.level = downcase(.level)' },
        { name: 'enrich', script: '.environment = "prod"' },
      ],
      sinks: ['logs_enriched', 'connector:archive', 'logs_archive'],
    });

    // Both sources → normalize → enrich → the two streams and the connector.
    expect(pipelineGraphStats(model)).toEqual({ nodes: 7, edges: 6 });
    expect(pipelineGraphStats(graph())).toEqual({ nodes: 2, edges: 1 });
    // Without steps, every source leads into every destination.
    expect(pipelineGraphStats(graph({ sources: ['a', 'b'], sinks: ['c', 'd'] }))).toEqual({ nodes: 4, edges: 4 });
  });
});

describe('pipelineStreams', () => {
  it('names every stream a saved pipeline reads and writes, its own first', () => {
    expect(
      pipelineStreams({
        id: 'p',
        name: 'p',
        source_stream: 'app_logs',
        target_stream: 'clean_logs',
        function_steps: { sources: ['nginx_logs', 'app_logs'], sinks: ['archive_logs'] },
      }),
    ).toEqual({ sources: ['app_logs', 'nginx_logs'], sinks: ['clean_logs', 'archive_logs'] });
    expect(pipelineStreams({ id: 'p', name: 'p' })).toEqual({ sources: [], sinks: [] });
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

  it('offers the streams of the pipeline signal, by name', () => {
    expect(
      pipelineStreamOptions(
        [stream('z_logs', 'logs'), stream('metrics_total', 'metrics'), stream('a_logs', 'logs')],
        'logs',
      ),
    ).toEqual(['a_logs', 'z_logs']);
  });

  it('keeps a name the pipeline already has, though the catalogue does not list it', () => {
    expect(pipelineStreamOptions([stream('app_logs', 'logs')], 'logs', 'legacy_logs')).toEqual([
      'legacy_logs',
      'app_logs',
    ]);
    expect(pipelineStreamOptions([stream('app_logs', 'logs')], 'logs', 'app_logs')).toEqual(['app_logs']);
  });
});

it('roundtrips realtime output settings and removes the schedule', () => {
  const model = graph({
    mode: 'realtime',
    sources: ['default'],
    sinks: ['default'],
    retainSource: true,
    transforms: [route({}, { kind: 'field', field: 'appname', prefix: 'app_', fallback: 'default' })],
  });
  const payload = pipelineInputFromGraph({ name: 'route', graph: model, cron: 'every:5m' });
  expect(payload.cron).toBe('');
  expect(payload.function_steps).toMatchObject({ mode: 'realtime', retain_source: true, sources: ['default'], sinks: ['default'] });
  expect((payload.function_steps as Record<string, unknown>).routing).toBeUndefined();

  const restored = pipelineGraphFromPipeline({ ...payload, id: 'one' });
  expect(restored).toMatchObject({ mode: 'realtime', retainSource: true, sources: ['default'], sinks: ['default'] });
  expect(restored.transforms).toEqual(model.transforms);
});

it('opens routing an earlier editor saved beside the steps as the routing step it is now', () => {
  const restored = pipelineGraphFromPipeline({
    id: 'old',
    name: 'old',
    source_stream: 'default',
    target_stream: 'default',
    function_steps: {
      mode: 'realtime',
      steps: [],
      routing: { kind: 'field', field: 'appname', prefix: 'app_', fallback: 'default', retain_source: true },
    },
  });

  expect(restored.retainSource).toBe(true);
  expect(restored.transforms).toEqual([
    route({ target: 'default' }, { kind: 'field', field: 'appname', prefix: 'app_', fallback: 'default', retain_source: true }),
  ]);
});

it('roundtrips interleaved built-in and VRL steps without generating a script for routing', () => {
  const model = graph({
    transforms: [
      { name: 'parse', kind: 'vrl', script: '.appname = "orders"' },
      route(),
      { name: 'enrich', kind: 'vrl', script: '.processed = true' },
    ],
  });
  expect(validateGraph(model)).toEqual([]);
  const payload = pipelineInputFromGraph({ name: 'ordered', graph: model, cron: 'every:5m' });
  const steps = (payload.function_steps as { steps: Record<string, unknown>[] }).steps;
  expect(steps[1]).toMatchObject({ kind: 'builtin', operation: 'route', target: 'orders' });
  expect(steps[1]).not.toHaveProperty('script');
  expect(pipelineGraphFromPipeline({ ...payload, id: 'one' }).transforms.map((step) => step.kind ?? 'vrl')).toEqual(['vrl', 'builtin', 'vrl']);
});
