import { AxiosError, AxiosHeaders } from 'axios';
import { describe, expect, it } from 'vitest';

import { endpointFilterCandidates, isUnknownTraceField, traceFieldQuery, tracesHref } from './endpointFilters';

const service = { field: 'service.name', op: '=', value: 'checkout' } as const;

describe('an endpoint found again in the trace store', () => {
  it('reads an HTTP endpoint as its method and route, under both names of the method field', () => {
    expect(endpointFilterCandidates('checkout', { name: 'GET /orders/{id}', kind: 'http' })).toEqual([
      [service, { field: 'http.request.method', op: '=', value: 'GET' }, { field: 'http.route', op: '=', value: '/orders/{id}' }],
      [service, { field: 'http.method', op: '=', value: 'GET' }, { field: 'http.route', op: '=', value: '/orders/{id}' }],
    ]);
    expect(endpointFilterCandidates('checkout', { name: 'POST', kind: 'http' })[0]).toEqual([
      service,
      { field: 'http.request.method', op: '=', value: 'POST' },
    ]);
  });

  it('splits an RPC endpoint at its last slash', () => {
    expect(endpointFilterCandidates('checkout', { name: 'grpc.health.v1.Health/Check', kind: 'rpc' })).toEqual([
      [service, { field: 'rpc.service', op: '=', value: 'grpc.health.v1.Health' }, { field: 'rpc.method', op: '=', value: 'Check' }],
    ]);
  });

  it('reads a messaging endpoint as its operation and destination', () => {
    const [current, earlier] = endpointFilterCandidates('checkout', { name: 'publish orders', kind: 'messaging' });
    expect(current).toEqual([
      service,
      { field: 'messaging.operation.name', op: '=', value: 'publish' },
      { field: 'messaging.destination.template', op: '=', value: 'orders' },
    ]);
    expect(earlier?.[1]).toEqual({ field: 'messaging.operation', op: '=', value: 'publish' });
  });

  it('reads any other span by its name, and nothing for the overflow row', () => {
    expect(endpointFilterCandidates('checkout', { name: 'process batch', kind: 'span' })).toEqual([
      [service, { field: 'name', op: '=', value: 'process batch' }],
    ]);
    expect(endpointFilterCandidates('checkout', { name: '__other__', kind: 'http' })).toEqual([]);
    expect(endpointFilterCandidates('checkout', { name: 'x', kind: 'other' })).toEqual([]);
  });

  it('writes the filters as the Traces page query and link', () => {
    const filters = [service, { field: 'name', op: '=' as const, value: "it's" }];
    expect(traceFieldQuery(filters)).toBe("service.name = 'checkout' AND name = 'it\\'s'");
    const href = tracesHref(filters, { from: 1_700_000_000_000_000, to: 1_700_000_060_000_000 }, 'spans');
    const params = new URLSearchParams(href.split('?')[1]);
    expect(href.startsWith('/traces?')).toBe(true);
    expect(params.get('q')).toBe("service.name = 'checkout' AND name = 'it\\'s'");
    expect(params.get('tab')).toBe('spans');
    expect(params.get('from')).toBe('2023-11-14T22:13:20.000Z');
  });

  it('passes over a spelling only when the trace store has no field by that name', () => {
    const refused = (status: number, message: string) =>
      new AxiosError(message, 'ERR_BAD_REQUEST', undefined, undefined, {
        status,
        statusText: '',
        data: { error: 'invalid', message },
        headers: {},
        config: { headers: new AxiosHeaders() },
      });

    expect(isUnknownTraceField(refused(400, 'unsupported trace filter field: http.method'))).toBe(true);
    expect(isUnknownTraceField(refused(400, 'invalid time range'))).toBe(false);
    expect(isUnknownTraceField(refused(503, 'unsupported trace filter field: http.method'))).toBe(false);
    expect(isUnknownTraceField(new Error('unsupported trace filter field: http.method'))).toBe(false);
  });
});
