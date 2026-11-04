import { describe, expect, it } from 'vitest';

import { traceUrlQueryStateFromParams, writeTraceQueryParams } from './urlState';

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
        new URLSearchParams({ q: "service.name = 'checkout'" }),
      ).fields,
    ).toBe("service.name = 'checkout'");
  });

  it('builds a field query from contextual URL parameters, naming the fields as the stream does', () => {
    expect(
      traceUrlQueryStateFromParams(
        new URLSearchParams({
          traceId: 'trace-1',
          service: "check'out",
        }),
      ).fields,
    ).toBe("trace_id = 'trace-1' AND service.name = 'check\\'out'");
    expect(
      traceUrlQueryStateFromParams(new URLSearchParams({ operation: 'GET /cart', status: 'ERROR' })).fields,
    ).toBe("name contains 'GET /cart' AND status_code = 'ERROR'");
  });
});

describe('writeTraceQueryParams', () => {
  it('says the applied query in the address, in place of what a link asked for', () => {
    const next = writeTraceQueryParams(
      new URLSearchParams('tab=spans&traceId=abc&service=checkout&sort=duration_desc'),
      'fields',
      " status_code = 'ERROR' ",
    );

    expect(Object.fromEntries(next)).toEqual({ tab: 'spans', sort: 'duration_desc', q: "status_code = 'ERROR'" });
    expect(traceUrlQueryStateFromParams(next)).toEqual({ mode: 'fields', fields: "status_code = 'ERROR'", sql: '' });
  });

  it('carries SQL in its own param, and no statement as none', () => {
    expect(writeTraceQueryParams(new URLSearchParams('q=x'), 'sql', 'SELECT 1').toString()).toBe('sql=SELECT+1');
    expect(writeTraceQueryParams(new URLSearchParams('sql=SELECT+1&tab=red'), 'fields', '').toString()).toBe('tab=red');
  });
});
