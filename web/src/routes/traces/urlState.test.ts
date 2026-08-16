import { describe, expect, it } from 'vitest';

import { traceUrlQueryStateFromParams } from './urlState';

describe('traceUrlQueryStateFromParams', () => {
  it('hydrates SQL Explore links into SQL mode', () => {
    expect(
      traceUrlQueryStateFromParams(
        new URLSearchParams({ sql: 'SELECT * FROM traces' }),
      ),
    ).toEqual({
      mode: 'sql',
      fields: '',
      sql: 'SELECT * FROM traces',
    });
  });

  it('keeps field-query and legacy query links compatible', () => {
    expect(
      traceUrlQueryStateFromParams(
        new URLSearchParams({ q: "service_name = 'checkout'" }),
      ).fields,
    ).toBe("service_name = 'checkout'");
  });

  it('builds a field query from contextual URL parameters', () => {
    expect(
      traceUrlQueryStateFromParams(
        new URLSearchParams({
          trace_id: 'trace-1',
          service: "check'out",
        }),
      ).fields,
    ).toBe("trace_id = 'trace-1' AND service_name = 'check\\'out'");
  });
});
