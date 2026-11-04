import { describe, expect, it } from 'vitest';

import { buildSignalJumps } from '@/shell/SignalReference';
import { decodeFilters, encodeFilters } from '@/shell/UrlHydration';

describe('buildSignalJumps', () => {
  const time = {
    from: '2026-07-25T10:55:00.000Z',
    to: '2026-07-25T11:05:00.184Z',
  };
  const source = { type: 'trace' as const, id: 'trace-checkout-1' };

  it('builds exact trace and span-log links with contextual metrics', () => {
    const jumps = buildSignalJumps(
      'span_id',
      'span-payment-1',
      time,
      {
        source,
        labels: {
          trace_id: source.id,
          span_id: 'span-payment-1',
          service_name: 'payment-service',
          operation_name: 'payment.authorize',
          environment: 'production',
        },
      },
    );

    // The logs are asked for the span by meaning: the Logs page writes the query in the
    // names of the stream it opens, which a link cannot know.
    const logs = new URL(jumps.find((jump) => jump.id === 'logs')!.to, 'https://molesignal.local');
    expect(logs.pathname).toBe('/logs');
    expect(logs.searchParams.get('q')).toBeNull();
    expect(logs.searchParams.get('traceId')).toBe('trace-checkout-1');
    expect(logs.searchParams.get('spanId')).toBe('span-payment-1');
    expect(logs.searchParams.get('service')).toBeNull();
    expect(logs.searchParams.get('operation')).toBeNull();
    expect(logs.searchParams.get('source')).toBe('trace');
    expect(logs.searchParams.get('sourceId')).toBe(source.id);
    expect(logs.searchParams.get('from')).toBe(time.from);
    expect(logs.searchParams.get('to')).toBe(time.to);
    expect(jumps.find((jump) => jump.id === 'logs')?.relation).toBe('exact');

    const metrics = new URL(jumps.find((jump) => jump.id === 'metrics')!.to, 'https://molesignal.local');
    expect(metrics.searchParams.get('promql')).toContain('service="payment-service"');
    expect(metrics.searchParams.get('promql')).toContain('operation="payment.authorize"');
    expect(metrics.searchParams.get('promql')).toContain('environment="production"');
    expect(metrics.searchParams.get('promql')).not.toContain('traceId');

    const traces = new URL(jumps.find((jump) => jump.id === 'traces')!.to, 'https://molesignal.local');
    expect(traces.pathname).toBe('/traces/trace-checkout-1');
    expect(traces.searchParams.get('traceId')).toBe('trace-checkout-1');
    expect(traces.searchParams.get('spanId')).toBe('span-payment-1');
    expect(traces.searchParams.get('from')).toBe(time.from);
    expect(traces.searchParams.get('to')).toBe(time.to);
    expect(jumps.find((jump) => jump.id === 'traces')?.relation).toBe('exact');
  });

  it('keeps a service pivot scoped to the service and environment', () => {
    const jumps = buildSignalJumps(
      'service',
      'payment-service',
      time,
      {
        source,
        labels: {
          trace_id: source.id,
          span_id: 'span-payment-1',
          service_name: 'payment-service',
          operation_name: 'payment.authorize',
          environment: 'production',
        },
      },
    );

    const logs = new URL(jumps.find((jump) => jump.id === 'logs')!.to, 'https://molesignal.local');
    expect(logs.searchParams.get('q')).toBeNull();
    expect(logs.searchParams.get('service')).toBe('payment-service');
    expect(logs.searchParams.get('environment')).toBe('production');
    expect(logs.searchParams.has('traceId')).toBe(false);
    expect(logs.searchParams.has('spanId')).toBe(false);
    expect(logs.searchParams.has('operation')).toBe(false);

    // A trace query names the service as the stream keeps it.
    const traces = new URL(jumps.find((jump) => jump.id === 'traces')!.to, 'https://molesignal.local');
    expect(traces.searchParams.get('q')).toBe("service.name = 'payment-service'");
  });

  it('names the fields of a span as the stream does, whatever label a pinned filter came from', () => {
    const jumps = buildSignalJumps('host', 'node-3', time, { source }, [
      { key: 'service_name', value: 'checkout', operator: '=' },
      { key: 'operation', value: 'GET /cart', operator: '!=' },
      { key: 'k8s.pod.name', value: 'pod-1', operator: '=' },
    ]);

    const traces = new URL(jumps.find((jump) => jump.id === 'traces')!.to, 'https://molesignal.local');
    expect(traces.searchParams.get('q')).toBe(
      "service.name = 'checkout' AND name != 'GET /cart' AND k8s.pod.name = 'pod-1'",
    );

    // The logs carry the pinned filters as they are: the Logs page applies each on the
    // field of the stream it queries.
    const logs = new URL(jumps.find((jump) => jump.id === 'logs')!.to, 'https://molesignal.local');
    expect(logs.searchParams.get('q')).toBeNull();
    expect(logs.searchParams.get('host')).toBe('node-3');
    expect(decodeFilters(logs.searchParams.get('filters')!)).toHaveLength(3);
  });

  it('round-trips inclusive and exclusion filters while accepting old links', () => {
    const filters = [
      { key: 'service', value: 'checkout', operator: '=' as const },
      { key: 'environment', value: 'staging', operator: '!=' as const },
    ];
    expect(decodeFilters(encodeFilters(filters))).toEqual(filters);
    expect(decodeFilters(JSON.stringify([['service', 'checkout']]))).toEqual([
      { key: 'service', value: 'checkout', operator: '=' },
    ]);
  });
});
