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
import { changesInPlace, defaultEnrichQuery } from '../PipelineGraph/model';
import { isProtectedField } from '../PipelineGraph/protectedFields';
import { compileProcessor, type ProcessorConfig } from '../processors/catalog';
import { isRealtimeSource, isWritableStream } from '../PipelineGraph/streamName';

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
  routing: { kind: 'fixed', field: '', prefix: '', fallback: 'default', retainSource: false, ...routing },
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

  it('reads the RUM streams and changes their custom parts as they arrive, but writes none of them', () => {
    expect(validateGraph(graph({ sources: ['rum_errors'], sinks: ['rum_copy'] }))).toEqual([]);
    expect(found(graph({ sinks: ['clean_logs', 'rum_views'] }))).toEqual([['error', 'rum_written', 'sink-1']]);
    expect(found(graph({ deadLetterStream: 'rum_vitals' }))).toEqual([['error', 'rum_written', undefined]]);
    expect(found(graph({ transforms: [route({ target: 'rum_errors' })] }))).toEqual([
      ['error', 'route_to_rum', 'transform-0'],
    ]);

    const step = (processor: ProcessorConfig): TransformStep => ({
      name: 'tidy',
      script: compileProcessor(processor),
      processor,
    });
    const set = (field: string) => step({ op: 'set', field, valueType: 'string', value: 'checkout' });
    const bots = step({ op: 'filter', action: 'drop', field: 'view.name', test: 'equals', value: 'bot' });
    const rum = (source: string, transforms: TransformStep[] = []) =>
      graph({ mode: 'realtime', sources: [source], sinks: [], transforms });

    // In place, in the custom parts; sessions are built from the views, and views are never dropped.
    expect(validateGraph(rum('rum_actions'))).toEqual([]);
    expect(found(rum('rum_sessions'))).toEqual([['error', 'rum_sessions_source', 'source-0']]);
    expect(validateGraph(rum('rum_views', [set('context.team')]))).toEqual([]);
    expect(found(rum('rum_views', [set('view.name')]))).toEqual([['error', 'protected_field', 'transform-0']]);
    expect(found(rum('rum_views', [bots]))).toEqual([['error', 'rum_keeps_rows', 'transform-0']]);
    expect(validateGraph(rum('rum_actions', [bots]))).toEqual([]);
  });

  it('reads the streams the platform writes, and writes none of them', () => {
    expect(validateGraph(graph({ sources: ['_audit'], sinks: ['siem_copy'] }))).toEqual([]);
    expect(found(graph({ sinks: ['_synthetic_results'] }))).toEqual([['error', 'platform_written', 'sink-0']]);
    expect(found(graph({ mode: 'realtime', sources: ['_audit'], sinks: ['audit_copy'] }))).toEqual([
      ['error', 'platform_source', 'source-0'],
    ]);
    expect(found(graph({ transforms: [route({ target: '_audit' })] }))).toEqual([
      ['error', 'route_to_platform', 'transform-0'],
    ]);
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

  it('keeps realtime logs to one stream in and one out', () => {
    const realtime = graph({ mode: 'realtime' });

    expect(validateGraph(realtime)).toEqual([]);
    expect(found({ ...realtime, sinks: ['clean_logs', 'connector:c1'] })).toEqual([
      ['error', 'realtime_nodes', undefined],
    ]);
    expect(found({ ...realtime, sources: ['app_logs', 'audit_logs'] })).toContainEqual([
      'error',
      'realtime_nodes',
      undefined,
    ]);
  });

  it('changes metrics and traces in their own stream, keeping what they rest on', () => {
    const remove = (fields: string[]): TransformStep => {
      const processor: ProcessorConfig = { op: 'remove', fields };
      return { name: 'tidy', script: compileProcessor(processor), processor };
    };

    for (const signalType of ['metrics', 'traces'] as const) {
      // No destination to choose: the events go back into the stream they came from.
      const inPlace = graph({ mode: 'realtime', signalType, sources: ['prod'], sinks: [] });

      expect(validateGraph(inPlace)).toEqual([]);
      expect(found({ ...inPlace, transforms: [route()] })).toEqual([['error', 'in_place_route', undefined]]);
      expect(pipelineInputFromGraph({ name: 'tidy', graph: inPlace, cron: '' })).toMatchObject({
        sourceStream: 'prod',
        targetStream: 'prod',
        functionSteps: { mode: 'realtime', signalType: signalType, sinks: ['prod'], retainSource: false },
      });
    }

    const metrics = graph({ mode: 'realtime', signalType: 'metrics', sources: ['prod'], sinks: [] });
    expect(validateGraph({ ...metrics, transforms: [remove(['pod', 'instance'])] })).toEqual([]);
    expect(validateGraph({ ...metrics, transforms: [remove(['pod', 'le'])] })).toEqual([
      { level: 'error', code: 'protected_field', params: { name: 'tidy', field: 'le' }, node: 'transform-0' },
    ]);

    const traces = graph({ mode: 'realtime', signalType: 'traces', sources: ['prod'], sinks: [] });
    expect(validateGraph({ ...traces, transforms: [remove(['db.statement'])] })).toEqual([]);
    expect(found({ ...traces, transforms: [remove(['parent_span_id'])] })).toEqual([
      ['error', 'protected_field', 'transform-0'],
    ]);
    // A scheduled pipeline writes new rows: nothing of the ones it reads is changed in place.
    expect(validateGraph(graph({ signalType: 'metrics', sources: ['prod'], sinks: ['prod_copy'], transforms: [remove(['le'])] }))).toEqual([]);
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
      // Every alert event, first in the order: kept for the alert mode, saved only by it.
      alert: { stage: 'open', policies: [], sources: [], severities: [], labels: [], time: null, order: 0 },
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
      sourceStream: 'app_logs',
      targetStream: 'clean_logs',
      functionSteps: {
        sources: ['audit_logs', 'app_logs'],
        sinks: ['archive_logs', 'clean_logs'],
        sinkConnectors: ['c1', 'c1'],
        steps: [],
      },
    });

    expect(opened.sources).toEqual(['app_logs', 'audit_logs']);
    expect(opened.sinks).toEqual(['clean_logs', 'archive_logs', 'connector:c1']);
    expect(opened.transforms).toEqual([]);
  });

  it('shows the steps that are saved and makes none up', () => {
    const open = (function_steps: unknown) =>
      pipelineGraphFromPipeline({ id: 'p', name: 'p', sourceStream: 'a', targetStream: 'b', functionSteps: function_steps })
        .transforms;

    expect(open(undefined)).toEqual([]);
    expect(open({ steps: [] })).toEqual([]);
    expect(open({ steps: [{ transformName: 'keep', script: '. = .' }] })).toEqual([{ name: 'keep', script: '. = .' }]);
    // Shapes older editors saved.
    expect(open([{ function_name: 'old', script: '.a = 1' }])).toEqual([{ name: 'old', script: '.a = 1' }]);
    expect(open({ transformName: 'single', script: '.b = 2' })).toEqual([{ name: 'single', script: '.b = 2' }]);
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
      sourceStream: 'app_logs',
      targetStream: 'clean_logs',
      cron: 'every:5m',
      lookbackSecs: 300,
      enabled: true,
      functionSteps: {
        mode: 'scheduled',
        signalType: 'logs',
        sources: ['app_logs'],
        sinks: ['clean_logs'],
        // A connector nobody chose is not one.
        sinkConnectors: ['c1'],
        sinkTables: [],
        delaySecs: 60,
        retry: { runRetries: 3, deliveryRetries: 3 },
        steps: [
          { transformName: 'parse', kind: 'vrl', script: '. = parse_json!(.message)' },
          // A step nobody named goes by its place.
          { transformName: 'step-2', kind: 'vrl', script: '.seen = true' },
        ],
      },
    });
  });

  it('saves no steps as no steps', () => {
    const payload = pipelineInputFromGraph({ name: 'copy', graph: graph(), cron: 'every:1h' });

    expect((payload.functionSteps as { steps: unknown[] }).steps).toEqual([]);
  });

  it('makes nothing up for what was left empty', () => {
    const payload = pipelineInputFromGraph({ name: 'new', graph: defaultPipelineGraph('logs'), cron: 'every:5m' });

    expect(payload.sourceStream).toBe('');
    expect(payload.targetStream).toBe('');
    expect(payload.functionSteps).toMatchObject({ sources: [], sinks: [], steps: [] });
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

    expect(payload.targetStream).toBe('misc');
    expect(payload.functionSteps).toMatchObject({ sinks: ['misc'], sinkConnectors: ['c1'] });
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

    expect(payload.targetStream).toBe('orders');
    expect(payload.functionSteps).toMatchObject({ sinks: ['orders', 'archive_logs', 'audit_logs'] });
  });

  it('sends every source and every stream, the first of each as the pipeline’s own', () => {
    const payload = pipelineInputFromGraph({
      name: 'fan',
      graph: graph({ sources: ['app_logs', ' audit_logs '], sinks: ['clean_logs', 'archive_logs'] }),
      cron: 'every:5m',
    });

    expect(payload).toMatchObject({ sourceStream: 'app_logs', targetStream: 'clean_logs' });
    expect(payload.functionSteps).toMatchObject({
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

    expect(payload.functionSteps).toMatchObject({
      delaySecs: 300,
      retry: { runRetries: 0, deliveryRetries: 5 },
      deadLetterStream: 'failed_events',
      dedupeField: 'trace_id',
    });
    expect(pipelineGraphFromPipeline({ ...payload, id: 'one' })).toMatchObject({
      retry: { runRetries: 0, deliveryRetries: 5 },
      deadLetterStream: 'failed_events',
      dedupeField: 'trace_id',
      delaySecs: 300,
    });
    // Blank is none: nothing is saved for it.
    const plain = pipelineInputFromGraph({ name: 'plain', graph: graph(), cron: 'every:5m' });
    expect(plain.functionSteps).not.toHaveProperty('dead_letter_stream');
    expect(plain.functionSteps).not.toHaveProperty('dedupe_field');
    // A realtime pipeline neither waits nor retries.
    const realtime = pipelineInputFromGraph({ name: 'live', graph: { ...model, mode: 'realtime' }, cron: '' });
    expect(realtime.functionSteps).not.toHaveProperty('retry');
    expect(realtime.functionSteps).not.toHaveProperty('delay_secs');
    expect(realtime.functionSteps).not.toHaveProperty('dead_letter_stream');
  });

  it('opens a saved value out of range as the default', () => {
    const opened = pipelineGraphFromPipeline({
      id: 'p',
      name: 'p',
      sourceStream: 'a',
      targetStream: 'b',
      functionSteps: { retry: { runRetries: 99, deliveryRetries: 'x' }, delaySecs: -5 },
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

    expect((payload.functionSteps as { steps: unknown[] }).steps).toEqual([
      {
        transformName: 'rename',
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
    const steps = payload.functionSteps as { steps: Array<Record<string, unknown>> };
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

    expect((payload.functionSteps as { steps: unknown[] }).steps).toEqual([
      { transformName: 'rename', kind: 'vrl', script: '. = .' },
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
        sourceStream: 'app_logs',
        targetStream: 'clean_logs',
        functionSteps: { sources: ['nginx_logs', 'app_logs'], sinks: ['archive_logs'] },
      }),
    ).toEqual({ sources: ['app_logs', 'nginx_logs'], sinks: ['clean_logs', 'archive_logs'] });
    expect(pipelineStreams({ id: 'p', name: 'p' })).toEqual({ sources: [], sinks: [] });
  });
});

describe('the streams a field offers', () => {
  it('leaves out of a destination what no pipeline can write', () => {
    expect(['app_logs', '_audit', '_synthetic_results', 'rum_views', '_molesignal'].filter(isWritableStream)).toEqual([
      'app_logs',
    ]);
  });

  it('leaves out of a realtime source what no pipeline can change as it arrives', () => {
    expect(['app_logs', 'rum_views', 'rum_sessions', '_audit', '_agent_model_traces'].filter(isRealtimeSource)).toEqual([
      'app_logs',
      'rum_views',
    ]);
  });
});

describe('pipelineStreamOptions', () => {
  const stream = (name: string, streamType: 'logs' | 'metrics' | 'traces') => ({
    id: name,
    label: name,
    name,
    streamType: streamType,
    type: streamType,
    schema: { fields: [] },
    retention: null,
    effectiveRetention: { days: 30 },
    settings: {
      description: null,
      indexRules: [],
      maxQueryRangeHours: null,
      queryable: true,
    },
    createdAt: 0,
    updatedAt: 0,
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
  expect(payload.functionSteps).toMatchObject({ mode: 'realtime', retainSource: true, sources: ['default'], sinks: ['default'] });
  expect((payload.functionSteps as Record<string, unknown>).routing).toBeUndefined();

  const restored = pipelineGraphFromPipeline({ ...payload, id: 'one' });
  expect(restored).toMatchObject({ mode: 'realtime', retainSource: true, sources: ['default'], sinks: ['default'] });
  expect(restored.transforms).toEqual(model.transforms);
});

it('opens routing an earlier editor saved beside the steps as the routing step it is now', () => {
  const restored = pipelineGraphFromPipeline({
    id: 'old',
    name: 'old',
    sourceStream: 'default',
    targetStream: 'default',
    functionSteps: {
      mode: 'realtime',
      steps: [],
      routing: { kind: 'field', field: 'appname', prefix: 'app_', fallback: 'default', retainSource: true },
    },
  });

  expect(restored.retainSource).toBe(true);
  expect(restored.transforms).toEqual([
    route({ target: 'default' }, { kind: 'field', field: 'appname', prefix: 'app_', fallback: 'default', retainSource: true }),
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
  const steps = (payload.functionSteps as { steps: Record<string, unknown>[] }).steps;
  expect(steps[1]).toMatchObject({ kind: 'builtin', operation: 'route', target: 'orders' });
  expect(steps[1]).not.toHaveProperty('script');
  expect(pipelineGraphFromPipeline({ ...payload, id: 'one' }).transforms.map((step) => step.kind ?? 'vrl')).toEqual(['vrl', 'builtin', 'vrl']);
});

describe('an alert pipeline', () => {
  const nightly = {
    mode: 'outside' as const,
    timezone: 'Asia/Shanghai',
    ranges: [{ weekdayMask: 0b011_1110, start: '09:00', end: '18:00' }],
  };
  const saved = {
    id: 'a1',
    name: 'prod alerts',
    sourceStream: '',
    targetStream: '',
    functionSteps: {
      mode: 'alert',
      alert: {
        sources: ['rule', 'email'],
        severities: ['critical'],
        labels: { env: 'prod' },
        time: nightly,
        order: 2,
      },
      steps: [
        { transformName: 'team', kind: 'vrl', script: '.labels.team = "payments"' },
        { transformName: 'explain', kind: 'ai_summary', prompt: 'Explain it', annotation: 'why' },
      ],
    },
  };

  it('opens with its conditions and steps, and no stream on either side', () => {
    const model = pipelineGraphFromPipeline(saved);

    expect(model.mode).toBe('alert');
    expect(model.sources).toEqual([]);
    expect(model.sinks).toEqual([]);
    // A source the editor does not know is left out rather than kept as a choice that cannot be made.
    expect(model.alert).toEqual({
      stage: 'open',
      policies: [],
      sources: ['rule'],
      severities: ['critical'],
      labels: [{ key: 'env', value: 'prod' }],
      time: nightly,
      order: 2,
    });
    expect(model.transforms[1]).toEqual({
      name: 'explain',
      kind: 'ai_summary',
      script: '',
      prompt: 'Explain it',
      annotation: 'why',
    });
    expect(model.transforms.map(stepType)).toEqual(['vrl', 'ai_summary']);
    expect(validateGraph(model)).toEqual([]);
    expect(pipelineGraphStats(model)).toEqual({ nodes: 4, edges: 3 });
  });

  it('is saved as its conditions and steps, reading and writing no stream', () => {
    const model = pipelineGraphFromPipeline(saved);
    model.alert.labels.push({ key: ' ', value: 'blank keys are left out' });

    expect(pipelineInputFromGraph({ name: 'prod alerts', graph: model, cron: 'every:5m', enabled: true })).toEqual({
      name: 'prod alerts',
      sourceStream: '',
      targetStream: '',
      cron: '',
      enabled: true,
      functionSteps: {
        mode: 'alert',
        alert: {
          stage: 'open',
          sources: ['rule'],
          severities: ['critical'],
          labels: { env: 'prod' },
          time: nightly,
          order: 2,
        },
        steps: [
          { transformName: 'team', kind: 'vrl', script: '.labels.team = "payments"' },
          { transformName: 'explain', kind: 'ai_summary', prompt: 'Explain it', annotation: 'why' },
        ],
      },
    });
  });

  it('has no routing or sampling, and an AI summary has to say what it asks', () => {
    const sample = { op: 'sample', percent: '10', by: '' } as const;
    const model = {
      ...pipelineGraphFromPipeline(saved),
      transforms: [
        route(),
        { name: 'ask', kind: 'ai_summary' as const, script: '', prompt: ' ', annotation: '' },
        { name: 'sample', script: compileProcessor(sample), processor: sample },
      ],
    };

    expect(found(model)).toEqual([
      ['error', 'alert_route', 'transform-0'],
      ['error', 'alert_prompt_missing', 'transform-1'],
      ['error', 'alert_sample', 'transform-2'],
    ]);
  });
});

describe('an alert pipeline before notifications', () => {
  const saved = {
    id: 'a2',
    name: 'quiet nights',
    sourceStream: '',
    targetStream: '',
    functionSteps: {
      mode: 'alert',
      alert: { stage: 'notify', policies: ['p-1'], sources: ['manual'], severities: [], labels: {}, order: 0 },
      steps: [{ transformName: 'quiet', kind: 'vrl', script: 'abort' }],
    },
  };

  it('opens and saves its stage, its policies and the incidents opened by hand', () => {
    const model = pipelineGraphFromPipeline(saved);

    expect(model.alert).toMatchObject({ stage: 'notify', policies: ['p-1'], sources: ['manual'] });
    expect(pipelineInputFromGraph({ name: 'quiet nights', graph: model, cron: '' }).functionSteps).toMatchObject({
      alert: { stage: 'notify', policies: ['p-1'], sources: ['manual'] },
    });
  });

  it('leaves its policies and manual incidents behind when it runs as the incident opens again', () => {
    const model = pipelineGraphFromPipeline(saved);
    model.alert.stage = 'open';

    const { alert } = pipelineInputFromGraph({ name: 'x', graph: model, cron: '' }).functionSteps as {
      alert: Record<string, unknown>;
    };

    expect(alert.stage).toBe('open');
    expect(alert.sources).toEqual([]);
    expect(alert).not.toHaveProperty('policies');
  });

  it('refuses a step that goes outside, whatever kind', () => {
    const model = {
      ...pipelineGraphFromPipeline(saved),
      transforms: [
        { name: 'ask', kind: 'ai_summary' as const, script: '', prompt: 'Why?' },
        { name: 'call', kind: 'http_enrich' as const, script: '', connectorId: 'c-1' },
        { name: 'find', kind: 'query_enrich' as const, script: '', query: { ...defaultEnrichQuery, stream: 's', statement: 'x' } },
      ],
    };

    expect(found(model)).toEqual([
      ['error', 'alert_notify_outside', 'transform-0'],
      ['error', 'alert_notify_outside', 'transform-1'],
      ['error', 'alert_notify_outside', 'transform-2'],
    ]);
  });
});

describe('steps that go outside an alert pipeline', () => {
  const saved = {
    id: 'a3',
    name: 'enrich',
    sourceStream: '',
    targetStream: '',
    functionSteps: {
      mode: 'alert',
      alert: {},
      steps: [
        { transformName: 'cmdb', kind: 'http_enrich', connectorId: 'c-1' },
        {
          transformName: 'errors',
          kind: 'query_enrich',
          language: 'promql',
          stream: 'node',
          streamType: 'metrics',
          statement: 'up{i="{{labels.host}}"}',
          windowSecs: 60,
          maxRows: 2,
          annotation: 'up',
        },
      ],
    },
  };

  it('open and save as they were', () => {
    const model = pipelineGraphFromPipeline(saved);

    expect(model.transforms.map(stepType)).toEqual(['http_enrich', 'query_enrich']);
    expect(model.transforms[0]).toMatchObject({ connectorId: 'c-1' });
    expect(model.transforms[1]!.query).toEqual({
      language: 'promql',
      stream: 'node',
      streamType: 'metrics',
      statement: 'up{i="{{labels.host}}"}',
      windowSecs: 60,
      maxRows: 2,
      annotation: 'up',
    });
    expect(found(model)).toEqual([]);

    const { steps } = pipelineInputFromGraph({ name: 'enrich', graph: model, cron: '' }).functionSteps as {
      steps: unknown[];
    };

    expect(steps).toEqual(saved.functionSteps.steps);
  });

  it('say what they lack', () => {
    const model = {
      ...pipelineGraphFromPipeline(saved),
      transforms: [
        { name: 'a', kind: 'http_enrich' as const, script: '', connectorId: '' },
        { name: 'b', kind: 'query_enrich' as const, script: '', query: defaultEnrichQuery },
        {
          name: 'c',
          kind: 'query_enrich' as const,
          script: '',
          query: { ...defaultEnrichQuery, stream: 'app_logs' },
        },
      ],
    };

    expect(found(model)).toEqual([
      ['error', 'alert_connector_missing', 'transform-0'],
      ['error', 'alert_query_stream_missing', 'transform-1'],
      ['error', 'alert_query_statement_missing', 'transform-2'],
    ]);
  });

  it('read what a saved step does not say as its default', () => {
    const model = pipelineGraphFromPipeline({
      ...saved,
      functionSteps: { mode: 'alert', steps: [{ transformName: 'q', kind: 'query_enrich' }] },
    });

    expect(model.transforms[0]!.query).toEqual(defaultEnrichQuery);
  });
});

describe('a realtime pipeline on profiles', () => {
  it('changes the metadata in place, and keeps where the object is', () => {
    const model = graph({ mode: 'realtime', signalType: 'profiles', sources: ['default'], sinks: [''] });

    expect(changesInPlace(model)).toBe(true);
    expect(isProtectedField('profiles', 'object_key')).toBe(true);
    expect(isProtectedField('profiles', 'service')).toBe(false);
    expect(isProtectedField('profiles', 'labels')).toBe(false);
  });
});

describe('extend tables as destinations', () => {
  it('want a table chosen, once each, and are saved beside the streams', () => {
    const model = graph({ sinks: ['clean_logs', 'table:host_owner', 'table:host_owner', 'table:'] });

    expect(found(model)).toEqual([
      ['warning', 'table_duplicate', 'sink-2'],
      ['error', 'table_unset', 'sink-3'],
    ]);

    const payload = pipelineInputFromGraph({ name: 'owners', graph: graph({ sinks: ['clean_logs', 'table:host_owner'] }), cron: 'every:5m' });
    const steps = payload.functionSteps as Record<string, unknown>;

    expect(payload.targetStream).toBe('clean_logs');
    expect(steps.sinks).toEqual(['clean_logs']);
    expect(steps.sinkTables).toEqual(['host_owner']);
    expect(pipelineGraphFromPipeline({ ...payload, id: 'p' }).sinks).toEqual(['clean_logs', 'table:host_owner']);
  });

  it('are none of a realtime pipeline', () => {
    expect(found(graph({ mode: 'realtime', sinks: ['clean_logs', 'table:host_owner'] }))).toEqual([
      ['error', 'realtime_nodes', undefined],
    ]);
  });
});
