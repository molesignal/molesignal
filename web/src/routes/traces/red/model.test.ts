import { describe, expect, it } from 'vitest';

import { parseTraceStatement, type TraceFieldDef } from '../fieldQueryModel';
import {
  dimensionFor,
  drillInto,
  drillSteps,
  everyBar,
  groupableFields,
  keepSteps,
  LATENCY_EDGES_NS,
  latencySlots,
  narrowDuration,
  narrowToFailures,
  nextDimension,
  rateUnitFor,
  sortGroups,
  withoutStep,
} from './model';

const FIELDS: TraceFieldDef[] = [
  { name: 'trace_id', dataType: 'utf8', physical: true, scope: 'span' },
  { name: 'span_id', dataType: 'utf8', physical: true, scope: 'span' },
  { name: 'name', dataType: 'utf8', physical: true, scope: 'span' },
  { name: 'service.name', dataType: 'utf8', physical: true, scope: 'span' },
  { name: 'http.status_code', dataType: 'int64', physical: true, scope: 'span' },
  { name: 'k8s.pod.name', dataType: 'utf8', physical: true, scope: 'span' },
  { name: 'cache.hit', dataType: 'bool', physical: true, scope: 'span' },
  { name: 'zone', dataType: 'utf8', physical: true, scope: 'span' },
  { name: 'start_time_unix_nano', dataType: 'int64', physical: true, scope: 'span' },
  { name: 'duration_ns', dataType: 'int64', physical: true, scope: 'trace_aggregate' },
  { name: 'attributes', dataType: 'json', physical: true, scope: 'span' },
  { name: 'span_count', dataType: 'int64', physical: false, scope: 'trace_aggregate' },
];

const filtersOf = (statement: string) => parseTraceStatement(statement, FIELDS).filters;

describe('groupableFields', () => {
  it('offers the fields that group requests, the drill order first and the rest by name', () => {
    expect(groupableFields(FIELDS).map((field) => field.name)).toEqual([
      'service.name',
      'name',
      'http.status_code',
      'k8s.pod.name',
      'cache.hit',
      'zone',
    ]);
  });
});

describe('dimensionFor', () => {
  it('keeps a field the stream has, and otherwise goes to the first the query does not hold to one value', () => {
    expect(dimensionFor('zone', FIELDS, [])).toBe('zone');
    expect(dimensionFor('nowhere', FIELDS, [])).toBe('service.name');
    expect(dimensionFor(null, FIELDS, filtersOf("service.name = 'api'"))).toBe('name');
  });

  it('has nothing to offer without a field that groups', () => {
    expect(dimensionFor(null, [FIELDS[0]!], [])).toBeNull();
  });
});

describe('nextDimension', () => {
  it('goes one field further down the drill order, past the ones already held', () => {
    expect(nextDimension('service.name', FIELDS, [])).toBe('name');
    expect(nextDimension('service.name', FIELDS, filtersOf("name = 'GET /'"))).toBe('http.status_code');
  });

  it('stays on a field outside the order when nothing in it is left', () => {
    const held = filtersOf("service.name = 'a' AND name = 'b' AND http.status_code = 200 AND k8s.pod.name = 'p'");

    expect(nextDimension('zone', FIELDS, held)).toBe('zone');
  });
});

describe('the drill path', () => {
  const statement = "service.name = \"api\" AND name contains 'cart' AND timeout";

  it('is the query read step by step', () => {
    expect(drillSteps(statement)).toEqual([
      { text: 'service.name = "api"', clause: { field: 'service.name', op: '=', value: 'api' } },
      { text: "name contains 'cart'", clause: { field: 'name', op: 'contains', value: 'cart' } },
      { text: 'timeout', clause: null },
    ]);
  });

  it('goes back up to a step, or leaves one out', () => {
    expect(keepSteps(statement, 0)).toBe('');
    expect(keepSteps(statement, 1)).toBe('service.name = "api"');
    expect(withoutStep(statement, 1)).toBe('service.name = "api" AND timeout');
  });

  it('goes down by a value written as its field reads it, once', () => {
    const field = (name: string) => FIELDS.find((candidate) => candidate.name === name);

    expect(drillInto('', field('service.name'), 'service.name', 'api')).toBe('service.name = "api"');
    expect(drillInto('timeout', field('http.status_code'), 'http.status_code', '500')).toBe(
      'timeout AND http.status_code = 500',
    );
    expect(drillInto('', field('cache.hit'), 'cache.hit', 'false')).toBe('cache.hit = false');
    expect(drillInto('service.name = "api"', field('service.name'), 'service.name', 'api')).toBe(
      'service.name = "api"',
    );
  });

  it('narrows to a stretch of durations in place of the one before', () => {
    expect(narrowDuration('service.name = "api" AND duration_ns >= 5', 100, 200)).toBe(
      'service.name = "api" AND duration_ns >= 100 AND duration_ns < 200',
    );
    // The first bar holds the shortest, the last the longest: each is open on its far side.
    expect(narrowDuration('', 0, 2)).toBe('duration_ns < 2');
    expect(narrowDuration('', 1_000_000_000, null)).toBe('duration_ns >= 1000000000');
  });

  it('narrows to the failed requests once', () => {
    expect(narrowToFailures('service.name = "api"')).toBe('service.name = "api" AND status_code = "ERROR"');
    expect(narrowToFailures("status_code = 'error'")).toBe("status_code = 'error'");
  });

  it('reads alternatives as one step and narrows them as a whole', () => {
    const either = 'service.name = "api" OR service.name = "web"';
    const status = FIELDS.find((candidate) => candidate.name === 'http.status_code');

    expect(drillSteps(either)).toEqual([{ text: either, clause: null }]);

    const drilled = drillInto(either, status, 'http.status_code', '500');
    expect(drilled).toBe('(service.name = "api" OR service.name = "web") AND http.status_code = 500');
    expect(drillSteps(drilled).map((step) => step.text)).toEqual([
      '(service.name = "api" OR service.name = "web")',
      'http.status_code = 500',
    ]);
    expect(withoutStep(drilled, 1)).toBe(either);
    expect(keepSteps(drilled, 1)).toBe(either);
    expect(narrowDuration(either, 100, null)).toBe(
      '(service.name = "api" OR service.name = "web") AND duration_ns >= 100',
    );
  });
});

describe('rateUnitFor', () => {
  it('says the highest rate as at least one', () => {
    expect(rateUnitFor([0.5, 3]).key).toBe('per_second');
    expect(rateUnitFor([0.02, 0.5]).key).toBe('per_minute');
    expect(rateUnitFor([0.0001]).key).toBe('per_hour');
    expect(rateUnitFor([]).key).toBe('per_hour');
  });
});

describe('everyBar', () => {
  it('puts each value in its bar and fills the rest', () => {
    const bars = { originTime: 1_000, bucketTime: 10, count: 4 };
    const points = [
      { startTime: 1_010, value: 3 },
      { startTime: 1_030, value: 7 },
      // Of other bars: left out.
      { startTime: 1_005, value: 9 },
      { startTime: 1_040, value: 9 },
    ];

    expect(everyBar(points, bars, (point) => point.value, 0)).toEqual([0, 3, 0, 7]);
  });
});

describe('latencySlots', () => {
  it('runs from the shortest bar that holds a request to the longest, the empty ones between included', () => {
    const bars = Array.from({ length: 12 }, (_, step) => ({
      fromNs: LATENCY_EDGES_NS[40 + step]!,
      toNs: LATENCY_EDGES_NS[41 + step]!,
      requests: step === 0 ? 4 : step === 11 ? 2 : 0,
      errors: step === 0 ? 1 : step === 11 ? null : 0,
    })).filter((bar) => bar.requests > 0);
    const slots = latencySlots(bars);

    expect(slots).toHaveLength(12);
    expect(slots[0]).toEqual({ fromNs: 1_000_000, toNs: 1_250_000, ok: 3, errors: 1 });
    expect(slots[1]).toEqual({ fromNs: 1_250_000, toNs: 1_600_000, ok: 0, errors: 0 });
    expect(slots[11]).toEqual({ fromNs: 12_500_000, toNs: 16_000_000, ok: 2, errors: 0 });
  });

  it('widens a narrow spread on both sides, within the edges', () => {
    const one = latencySlots([{ fromNs: 1_000_000, toNs: 1_250_000, requests: 1, errors: 0 }]);

    expect(one).toHaveLength(12);
    expect(one.findIndex((slot) => slot.ok === 1)).toBe(6);

    const shortest = latencySlots([{ fromNs: 0, toNs: 125, requests: 1, errors: 0 }]);

    expect(shortest[0]).toMatchObject({ fromNs: 0, toNs: 125, ok: 1 });
    expect(shortest).toHaveLength(12);
  });

  it('has its edges where the backend counts them', () => {
    expect(LATENCY_EDGES_NS.slice(0, 6)).toEqual([100, 125, 160, 200, 250, 315]);
    expect(LATENCY_EDGES_NS).toHaveLength(101);
    expect(LATENCY_EDGES_NS.at(-1)).toBe(1e12);
  });
});

describe('sortGroups', () => {
  const group = (value: string, requests: number, errors: number | null, p95: number | null) => ({
    value,
    requests,
    errors,
    p50Ns: null,
    p95Ns: p95,
    p99Ns: null,
  });
  const groups = [group('a', 10, 0, 5), group('b', 50, 5, 1), group('c', 20, 4, null), group('d', 5, null, 9)];

  it('orders by the figure in front, what cannot be told last', () => {
    expect(sortGroups(groups, 'rate').map((item) => item.value)).toEqual(['b', 'c', 'a', 'd']);
    expect(sortGroups(groups, 'errors').map((item) => item.value)).toEqual(['c', 'b', 'a', 'd']);
    expect(sortGroups(groups, 'duration').map((item) => item.value)).toEqual(['d', 'a', 'b', 'c']);
  });
});
