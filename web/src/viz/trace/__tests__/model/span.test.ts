import { describe, expect, it } from 'vitest';

import { formatTraceDurationNs } from '../../duration';
import {
  attributeGroup,
  groupAttributes,
  httpStatusOf,
  httpStatusTone,
  requestOutcome,
  spanExceptions,
  spanKindName,
  splitAttributes,
} from '../../model/span';
import { span } from './fixture';

describe('span facts', () => {
  it('reads the HTTP status under either name, stored as a number or as digits', () => {
    expect(httpStatusOf({ attributes: { 'http.response.status_code': 503 } })).toBe(503);
    expect(httpStatusOf({ attributes: { 'http.status_code': '404' } })).toBe(404);
    expect(httpStatusOf({ attributes: { 'http.status_code': '***' } })).toBeNull();
    expect([200, 302, 404, 503].map(httpStatusTone)).toEqual(['ok', 'redirect', 'client', 'server']);
  });

  it('reads a refused request as gone wrong, though not as failed, and a 5xx as failed', () => {
    expect(requestOutcome(false, 200)).toBe('ok');
    expect(requestOutcome(false, 302)).toBe('ok');
    expect(requestOutcome(false, undefined)).toBe('ok');
    expect(requestOutcome(false, 403)).toBe('refused');
    expect(requestOutcome(false, 503)).toBe('failed');
    expect(requestOutcome(true, 200)).toBe('failed');
    expect(requestOutcome(true, null)).toBe('failed');
  });

  it('names a kind from its number, its digits or its name', () => {
    expect(spanKindName(2)).toBe('server');
    expect(spanKindName('3')).toBe('client');
    expect(spanKindName('SPAN_KIND_CONSUMER')).toBe('consumer');
    expect(spanKindName(9)).toBe('unspecified');
  });

  it('reads the exceptions a span recorded', () => {
    const failed = span('a', null, 'x', 'a', 0, 1, {
      events: [
        { tsNs: 5, name: 'retry', attributes: {}, droppedAttributesCount: 0 },
        {
          tsNs: 9,
          name: 'exception',
          attributes: { 'exception.type': 'Timeout', 'exception.message': 'gave up', 'exception.stacktrace': 'at f()' },
          droppedAttributesCount: 0,
        },
      ],
    });

    expect(spanExceptions(failed)).toEqual([{ type: 'Timeout', message: 'gave up', stacktrace: 'at f()', tsNs: 9 }]);
  });

  it('says a resource’s tag once, under the resource', () => {
    const flattened = span('a', null, 'x', 'a', 0, 1, {
      resource: { attributes: { 'host.name': 'node-1', 'service.version': '1.2' }, dropped_attributes_count: 0 },
      attributes: { 'host.name': 'node-1', 'db.system': 'postgresql', 'service.version': '2.0', 'molesignal.trace.span_count': 3 },
    });

    const { attributes, resource } = splitAttributes(flattened);
    // The span's own `service.version` differs from the resource's: both are said.
    expect(attributes.map(([key]) => key)).toEqual(['db.system', 'service.version']);
    expect(resource.map(([key]) => key)).toEqual(['host.name', 'service.version']);
  });

  it('groups tags by the part of a span they describe', () => {
    expect(['http.route', 'url.path', 'db.statement', 'messaging.system', 'exception.type', 'k8s.pod.name', 'foo'].map(attributeGroup)).toEqual([
      'http',
      'http',
      'database',
      'messaging',
      'error',
      'container',
      'other',
    ]);
    expect(groupAttributes([['foo', 1], ['db.system', 'pg'], ['http.route', '/']]).map(({ group }) => group)).toEqual([
      'http',
      'database',
      'other',
    ]);
  });
});

describe('formatTraceDurationNs', () => {
  it('reads a second and longer in seconds and minutes', () => {
    expect(formatTraceDurationNs(3_129_000_000)).toBe('3.13 s');
    expect(formatTraceDurationNs(12_340_000_000)).toBe('12.3 s');
    expect(formatTraceDurationNs(921_069_636_000)).toBe('15 min 21 s');
    expect(formatTraceDurationNs(7_199_000_000_000)).toBe('1 h 59 min');
    expect(formatTraceDurationNs(250_000_000)).toBe('250 ms');
    expect(formatTraceDurationNs(41_200)).toBe('41.2 μs');
  });
});
