import { describe, expect, it } from 'vitest';

import { otlpAnyValue, toOtlpTrace } from '../../model/otlp';
import { traceFileName, traceFileText } from '../../view/Export';
import { span, T0, trace } from './fixture';

const MS = 1_000_000;

function exported() {
  return trace([
    span('root', null, 'checkout', 'POST /orders', 0, 100, {
      kind: 2,
      resource: { attributes: { 'service.name': 'checkout', 'host.name': 'node-1' }, dropped_attributes_count: 0 },
      scope: { name: 'http', version: '1.0', attributes: {}, dropped_attributes_count: 0 },
      // The store copies a resource's tags into the span's row: they go back to the resource.
      attributes: { 'host.name': 'node-1', 'http.route': '/orders', 'molesignal.trace.depth': 0 },
    }),
    span('db', 'root', 'checkout', 'SELECT orders', 10, 40, {
      kind: 3,
      resource: { attributes: { 'service.name': 'checkout', 'host.name': 'node-1' }, dropped_attributes_count: 0 },
      scope: { name: 'sql', version: '2.1', attributes: {}, dropped_attributes_count: 0 },
      events: [{ tsNs: T0 + 20 * MS, name: 'retry', attributes: { attempt: 2 }, droppedAttributesCount: 0 }],
    }),
    span('pay', 'root', 'payments', 'POST /charge', 50, 95, {
      kind: 'SPAN_KIND_SERVER' as unknown as number,
      status: 'ERROR',
      statusMessage: 'card declined',
      links: [{ trace_id: 'other', span_id: 'x1', trace_state: '', flags: 1, attributes: {}, dropped_attributes_count: 0 }],
    }),
  ]);
}

describe('toOtlpTrace', () => {
  it('holds the spans of one resource together, and within it those of one scope', () => {
    const otlp = toOtlpTrace(exported());

    expect(otlp.resourceSpans).toHaveLength(2);
    const [checkout, payments] = otlp.resourceSpans;
    expect(checkout!.resource.attributes).toEqual([
      { key: 'host.name', value: { stringValue: 'node-1' } },
      { key: 'service.name', value: { stringValue: 'checkout' } },
    ]);
    expect(checkout!.scopeSpans.map((scope) => [scope.scope.name, scope.spans.map((item) => item.spanId)])).toEqual([
      ['http', ['root']],
      ['sql', ['db']],
    ]);
    // A resource that does not name its service is given the span's.
    expect(payments!.resource.attributes).toEqual([{ key: 'service.name', value: { stringValue: 'payments' } }]);
  });

  it('writes a span as OTLP/JSON does: ids, kinds and codes as numbers, nanoseconds as decimal strings', () => {
    const [checkout, payments] = toOtlpTrace(exported()).resourceSpans;
    const root = checkout!.scopeSpans[0]!.spans[0]!;
    const db = checkout!.scopeSpans[1]!.spans[0]!;
    const pay = payments!.scopeSpans[0]!.spans[0]!;

    expect(root).toMatchObject({
      traceId: 'trace-1',
      spanId: 'root',
      name: 'POST /orders',
      kind: 2,
      startTimeUnixNano: String(T0),
      endTimeUnixNano: String(T0 + 100 * MS),
      status: { code: 1 },
    });
    expect(root.parentSpanId).toBeUndefined();
    // The span's own tags only: not its resource's, nor the store's own.
    expect(root.attributes).toEqual([{ key: 'http.route', value: { stringValue: '/orders' } }]);
    expect(db).toMatchObject({ parentSpanId: 'root', kind: 3 });
    expect(db.events).toEqual([
      { timeUnixNano: String(T0 + 20 * MS), name: 'retry', attributes: [{ key: 'attempt', value: { intValue: '2' } }], droppedAttributesCount: 0 },
    ]);
    expect(pay).toMatchObject({ kind: 2, status: { code: 2, message: 'card declined' } });
    expect(pay.links).toEqual([{ traceId: 'other', spanId: 'x1', traceState: '', flags: 1, attributes: [], droppedAttributesCount: 0 }]);
  });

  it('keeps an epoch past what a number holds exactly, and the duration to the nanosecond', () => {
    // Past 2^53 a number holds multiples of a power of two only: this one exactly.
    const late = trace([span('a', null, 'web', 'GET /', 0, 1, { startNs: 2 ** 60 + 2 ** 10, durationNs: 1_234 })]);
    const only = toOtlpTrace(late).resourceSpans[0]!.scopeSpans[0]!.spans[0]!;

    expect(only.startTimeUnixNano).toBe((2n ** 60n + 1_024n).toString());
    expect(BigInt(only.endTimeUnixNano) - BigInt(only.startTimeUnixNano)).toBe(1_234n);
  });

  it('writes every kind of value', () => {
    expect(otlpAnyValue(1.5)).toEqual({ doubleValue: 1.5 });
    expect(otlpAnyValue(true)).toEqual({ boolValue: true });
    expect(otlpAnyValue(null)).toEqual({});
    expect(otlpAnyValue(['a', 2])).toEqual({ arrayValue: { values: [{ stringValue: 'a' }, { intValue: '2' }] } });
    expect(otlpAnyValue({ b: 'x' })).toEqual({ kvlistValue: { values: [{ key: 'b', value: { stringValue: 'x' } }] } });
  });
});

describe('trace files', () => {
  it('names a file after the trace, and writes the answer of the trace API as it came', () => {
    expect(traceFileName('4bf92f3577b34da6a3ce929d0e0e4736', 'json')).toBe('trace-4bf92f3577b34da6a3ce929d0e0e4736.json');
    expect(traceFileName('a/b c', 'otlp')).toBe('trace-a-b-c.otlp.json');

    const answer = exported();
    expect(JSON.parse(traceFileText(answer, 'json'))).toEqual(answer);
    expect(JSON.parse(traceFileText(answer, 'otlp')).resourceSpans).toHaveLength(2);
  });
});
