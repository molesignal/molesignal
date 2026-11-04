import { describe, expect, it } from 'vitest';

import { keyAttributes, logTraceOf } from './keyAttributes';

describe('the attributes a log is told by', () => {
  it('finds each meaning once, under the name the record gives it', () => {
    const record = {
      service: { name: 'checkout', version: '2.1' },
      host: 'node-1',
      deployment: { environment: { name: 'prod' } },
      trace_id: '4bf92f3577b34da6a3ce929d0e0e4736',
      span_id: '00f067aa0ba902b7',
      message: 'paid',
      peer: { service: 'billing' },
    };

    expect(keyAttributes(record)).toEqual([
      { key: 'service', field: 'service.name', value: 'checkout' },
      { key: 'host', field: 'host', value: 'node-1' },
      { key: 'env', field: 'deployment.environment.name', value: 'prod' },
      { key: 'version', field: 'service.version', value: '2.1' },
      { key: 'trace_id', field: 'trace_id', value: '4bf92f3577b34da6a3ce929d0e0e4736' },
      { key: 'span_id', field: 'span_id', value: '00f067aa0ba902b7' },
    ]);
  });

  it('leaves out empty values and fields that only end in a meaning without a dot', () => {
    expect(keyAttributes({ service: '', subservice: 'x', env: null, hostname: 'h' })).toEqual([{ key: 'host', field: 'hostname', value: 'h' }]);
  });

  it('reads the trace of a log only from an id that looks like one', () => {
    expect(logTraceOf({ trace_id: '4bf92f3577b34da6a3ce929d0e0e4736', span_id: '00f067aa0ba902b7' })).toEqual({
      traceId: '4bf92f3577b34da6a3ce929d0e0e4736',
      spanId: '00f067aa0ba902b7',
    });
    expect(logTraceOf({ trace_id: 'not-a-trace' })).toBeNull();
    expect(logTraceOf({ message: 'x' })).toBeNull();
  });
});
