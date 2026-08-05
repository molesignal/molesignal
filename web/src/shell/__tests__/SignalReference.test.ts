import { describe, expect, it } from 'vitest';

import { buildSignalJumps } from '@/shell/SignalReference';
import { decodeFilters, encodeFilters } from '@/shell/UrlHydration';

describe('buildSignalJumps', () => {
  const time = {
    from: '2026-07-25T10:55:00.000Z',
    to: '2026-07-25T11:05:00.184Z',
  };
  const source = { type: 'trace' as const, id: 'trace-checkout-1' };

  it('builds exact span logs and contextual metrics / similar traces', () => {
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

    const logs = new URL(jumps.find((jump) => jump.id === 'logs')!.to, 'https://molesignal.local');
    expect(logs.pathname).toBe('/logs');
    expect(logs.searchParams.get('q')).toContain("trace_id = 'trace-checkout-1'");
    expect(logs.searchParams.get('q')).toContain("span_id = 'span-payment-1'");
    expect(logs.searchParams.get('source')).toBe('trace');
    expect(logs.searchParams.get('source_id')).toBe(source.id);
    expect(logs.searchParams.get('from')).toBe(time.from);
    expect(logs.searchParams.get('to')).toBe(time.to);
    expect(jumps.find((jump) => jump.id === 'logs')?.relation).toBe('exact');

    const metrics = new URL(jumps.find((jump) => jump.id === 'metrics')!.to, 'https://molesignal.local');
    expect(metrics.searchParams.get('promql')).toContain('service="payment-service"');
    expect(metrics.searchParams.get('promql')).toContain('operation="payment.authorize"');
    expect(metrics.searchParams.get('promql')).toContain('environment="production"');
    expect(metrics.searchParams.get('promql')).not.toContain('trace_id');

    const traces = new URL(jumps.find((jump) => jump.id === 'traces')!.to, 'https://molesignal.local');
    expect(traces.searchParams.get('q')).toContain("service_name = 'payment-service'");
    expect(traces.searchParams.get('q')).toContain("operation_name contains 'payment.authorize'");
    expect(traces.searchParams.get('q')).not.toContain('trace_id');
    expect(traces.searchParams.get('q')).not.toContain('span_id');
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
    expect(logs.searchParams.get('q')).toContain("service = 'payment-service'");
    expect(logs.searchParams.get('q')).toContain("environment = 'production'");
    expect(logs.searchParams.get('q')).not.toContain('trace_id');
    expect(logs.searchParams.get('q')).not.toContain('span_id');
    expect(logs.searchParams.get('q')).not.toContain('operation');
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
