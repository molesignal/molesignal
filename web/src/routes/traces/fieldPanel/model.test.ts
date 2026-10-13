import { describe, expect, it } from 'vitest';

import {
  describeTraceValue,
  isTraceFieldExpandable,
  isTraceFieldMeasure,
  isTraceListField,
  traceFieldValue,
  traceLevelFieldValues,
  traceValueKind,
  type TraceFieldRecord,
} from './model';

const trace = (id: string, durationMs: number, spans: number, errors = 0): TraceFieldRecord => ({
  id,
  op: 'GET /',
  service: 'web',
  startNs: 1_786_704_600_000_000_000,
  durationMs,
  spans,
  errors,
});

describe('the values a trace has one of', () => {
  const traces = [trace('t-1', 442, 11), trace('t-2', 2450, 12, 6), trace('t-3', 5.5, 11)];

  it('lists ids in the order of the results', () => {
    expect(traceLevelFieldValues(traces, 'trace_id')).toEqual({
      traces: 3,
      distinct: 3,
      samples: 3,
      top: [{ value: 't-1', traces: 1 }, { value: 't-2', traces: 1 }, { value: 't-3', traces: 1 }],
    });
  });

  it('counts the traces that share a count, most common first', () => {
    const values = traceLevelFieldValues(traces, 'span_count');

    expect(values.top).toEqual([{ value: '11', traces: 2 }, { value: '12', traces: 1 }]);
    expect(values.range).toEqual({ min: '11', max: '12', avg: '11' });
  });

  it('gives a duration in the nanoseconds a filter takes', () => {
    const values = traceLevelFieldValues(traces, 'duration_ns');

    expect(values.top.map((item) => item.value)).toEqual(['442000000', '2450000000', '5500000']);
    expect(values.range).toEqual({ min: '5500000', max: '2450000000', avg: '965833333' });
  });

  it('has nothing to say of no traces', () => {
    expect(traceLevelFieldValues([], 'error_count')).toEqual({ traces: 0, distinct: 0, samples: 0, top: [] });
  });
});

describe('a measure and a code', () => {
  const values = (distinct: number, samples: number, shown: number) => ({
    traces: 20,
    distinct,
    samples,
    top: Array.from({ length: shown }, (_, index) => ({ value: String(index), traces: 1 })),
    range: { min: '0', max: '9', avg: '4' },
  });

  it('reads a number that is nearly never the same twice as a range', () => {
    expect(isTraceFieldMeasure(values(55, 55, 8))).toBe(true);
  });

  it('reads a number with a few values as a list, however many spans carry it', () => {
    expect(isTraceFieldMeasure(values(3, 400, 3))).toBe(false);
    // Nine status codes over four hundred spans are still codes.
    expect(isTraceFieldMeasure(values(9, 400, 8))).toBe(false);
  });

  it('never reads text as a range', () => {
    expect(isTraceFieldMeasure({ traces: 20, distinct: 55, samples: 55, top: [] })).toBe(false);
  });
});

describe('a value as it is shown', () => {
  const micros = (value: number) => `at ${value}`;

  it('goes by the name and type of its field', () => {
    expect(traceValueKind({ name: 'duration_ns', dataType: 'int64' })).toBe('duration_ns');
    expect(traceValueKind({ name: 'db.transaction.duration_ms', dataType: 'float64' })).toBe('duration_ms');
    expect(traceValueKind({ name: 'start_time_unix_nano', dataType: 'int64' })).toBe('time_ns');
    expect(traceValueKind({ name: 'seen_at', dataType: 'timestamp' })).toBe('time_micros');
    expect(traceValueKind({ name: 'kind', dataType: 'int64' })).toBe('span_kind');
    expect(traceValueKind({ name: 'http.status_code', dataType: 'int64' })).toBe('plain');
    // A text field that happens to be called a duration is still text.
    expect(traceValueKind({ name: 'duration_ns', dataType: 'utf8' })).toBe('plain');
  });

  it('puts a duration in its natural unit and a moment as a time', () => {
    expect(describeTraceValue('duration_ns', '442000000', micros)).toBe('442 ms');
    expect(describeTraceValue('duration_ms', '12.5', micros)).toBe('12.5 ms');
    expect(describeTraceValue('time_ns', '1791701310072074000', micros)).toBe('at 1791701310072074');
    expect(describeTraceValue('time_micros', '1791701313537673', micros)).toBe('at 1791701313537673');
  });

  it('names a span kind beside its number, and leaves what it cannot read', () => {
    expect(describeTraceValue('span_kind', '2', micros)).toBe('2 · SERVER');
    expect(describeTraceValue('span_kind', '9', micros)).toBe('9');
    expect(describeTraceValue('duration_ns', 'n/a', micros)).toBe('n/a');
    expect(describeTraceValue('plain', '0042', micros)).toBe('0042');
  });
});

describe('whether a field can be opened', () => {
  const field = (name: string, dataType: 'utf8' | 'json' | 'int64') => ({ name, dataType, physical: true, scope: 'span' as const });

  it('is never so for a document', () => {
    expect(isTraceFieldExpandable(field('attributes', 'json'), true)).toBe(false);
  });

  it('needs a stream to read a field of a span, and none for what is on the rows', () => {
    expect(isTraceFieldExpandable(field('span_id', 'utf8'), true)).toBe(true);
    expect(isTraceFieldExpandable(field('span_id', 'utf8'), false)).toBe(false);
    expect(isTraceFieldExpandable(field('span_count', 'int64'), false)).toBe(true);
  });
});

describe('what a picked trace can show', () => {
  it('is what its row in the list carries', () => {
    const picked = trace('t-1', 442, 11, 2);

    expect(traceFieldValue(picked, 'service.name')).toBe('web');
    expect(traceFieldValue(picked, 'error_count')).toBe('2');
    expect(isTraceListField('duration_ns')).toBe(true);
  });

  it('does not make up a status for a trace from its error count', () => {
    // A span's status is OK, ERROR or UNSET; a row of the list does not say which.
    expect(traceFieldValue(trace('t-1', 442, 11, 2), 'status_code')).toBe('');
    expect(isTraceListField('status_code')).toBe(false);
  });
});
