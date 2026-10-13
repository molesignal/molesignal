import { describe, expect, it } from 'vitest';

import { MINUTE, NOW, checkRow, result, runs } from './__tests__/builders';
import {
  availability,
  availabilityTone,
  availabilityTrend,
  checkFamily,
  checksNeedingAttention,
  checksVerdict,
  decidedRuns,
  lastFailureAt,
  latestError,
  runStrip,
  runningChecks,
  stateCounts,
  troubledLocationIds,
  unhealthySince,
} from './summary';

describe('checks verdict', () => {
  const verdictOf = (states: Array<Parameters<typeof checkRow>[0]>) =>
    checksVerdict(stateCounts(runningChecks(states.map((row) => checkRow(row)))));

  it('is the worst thing a running check says', () => {
    expect(
      verdictOf([
        { monitor: { id: 'a', state: 'healthy' } },
        { monitor: { id: 'b', state: 'degraded' } },
        { monitor: { id: 'c', state: 'failing' } },
      ]),
    ).toBe('failing');
    expect(
      verdictOf([
        { monitor: { id: 'a', state: 'healthy' } },
        { monitor: { id: 'b', state: 'flaky' } },
      ]),
    ).toBe('attention');
    expect(verdictOf([{ monitor: { id: 'a', state: 'healthy' } }])).toBe('passing');
  });

  it('does not call the module unwell for a check that has not reported yet', () => {
    expect(
      verdictOf([
        { monitor: { id: 'a', state: 'healthy' } },
        { monitor: { id: 'b', state: 'unknown' } },
      ]),
    ).toBe('passing');
    expect(verdictOf([{ monitor: { id: 'a', state: 'unknown' } }])).toBe('waiting');
  });

  it('leaves paused checks out: their last state is not what they are doing now', () => {
    expect(
      verdictOf([
        { monitor: { id: 'a', state: 'failing', lifecycle: 'paused' } },
        { monitor: { id: 'b', state: 'healthy' } },
      ]),
    ).toBe('passing');
    expect(verdictOf([{ monitor: { id: 'a', state: 'failing', lifecycle: 'paused' } }])).toBe('idle');
    expect(verdictOf([])).toBe('idle');
  });
});

describe('check families', () => {
  it('groups protocols the way a reader thinks of them', () => {
    expect(checkFamily('http')).toBe('api');
    expect(checkFamily('browser')).toBe('browser');
    expect(checkFamily('heartbeat')).toBe('heartbeat');
    for (const kind of ['tcp', 'ssh', 'dns', 'icmp', 'tls', 'grpc'] as const) {
      expect(checkFamily(kind)).toBe('network');
    }
  });
});

describe('runs', () => {
  it('counts only scheduled runs that came to a conclusion', () => {
    const results = [
      ...runs(['healthy', 'failing', 'unknown', 'skipped']),
      result({ outcome: 'failing', is_test: true, started_at: NOW }),
    ];

    expect(decidedRuns(results).map((run) => run.outcome)).toEqual(['healthy', 'failing']);
    expect(availability(results)).toBe(0.5);
    expect(availability([])).toBeUndefined();
  });

  it('dates trouble from the first run of the present streak', () => {
    // Latest first: failed three times in a row after a pass.
    const failing = runs(['failing', 'flaky', 'failing', 'healthy', 'failing']);

    expect(unhealthySince(failing)).toBe(NOW - 3 * MINUTE);
    expect(unhealthySince(runs(['healthy', 'failing']))).toBeUndefined();
    expect(unhealthySince([])).toBeUndefined();
    expect(lastFailureAt(runs(['healthy', 'healthy', 'degraded']))).toBe(NOW - 3 * MINUTE);
    expect(lastFailureAt(runs(['healthy', 'healthy']))).toBeUndefined();
  });

  it('takes the reason from the latest run that did not pass', () => {
    const results = [
      result({ outcome: 'healthy', started_at: NOW - MINUTE }),
      result({
        outcome: 'failing',
        started_at: NOW - 2 * MINUTE,
        attempts: [
          {
            number: 1,
            started_at: 0,
            finished_at: 1,
            outcome: 'failing',
            timing: {},
            error_category: 'timeout',
            error_message: 'connect timed out',
            metadata: {},
          },
        ],
      }),
      result({
        outcome: 'failing',
        started_at: NOW - 3 * MINUTE,
        attempts: [
          {
            number: 1,
            started_at: 0,
            finished_at: 1,
            outcome: 'failing',
            timing: {},
            error_message: 'older reason',
            metadata: {},
          },
        ],
      }),
    ];

    expect(latestError(results)).toBe('connect timed out');
    expect(latestError(runs(['healthy']))).toBeUndefined();
  });

  it('falls back to the failed assertion, then to the kind of error', () => {
    expect(
      latestError([
        result({
          outcome: 'degraded',
          assertions: [
            { assertion_id: 'a1', severity: 'warning', passed: false, message: 'duration 912 ms > 500 ms' },
          ],
        }),
      ]),
    ).toBe('duration 912 ms > 500 ms');
    expect(
      latestError([
        result({
          outcome: 'failing',
          attempts: [
            { number: 1, started_at: 0, finished_at: 1, outcome: 'failing', timing: {}, error_category: 'dns', metadata: {} },
          ],
        }),
      ]),
    ).toBe('dns');
  });

  it('names the locations whose latest run did not pass', () => {
    const results = [
      result({ location_id: 'loc-us', outcome: 'healthy', started_at: NOW - MINUTE }),
      result({ location_id: 'loc-us', outcome: 'failing', started_at: NOW - 2 * MINUTE }),
      result({ location_id: 'loc-eu', outcome: 'failing', started_at: NOW - MINUTE }),
      result({ location_id: 'loc-ap', outcome: 'flaky', started_at: NOW - 3 * MINUTE }),
    ];

    expect(troubledLocationIds(results).sort()).toEqual(['loc-ap', 'loc-eu']);
  });

  it('draws the latest runs oldest first and no more than asked for', () => {
    const strip = runStrip(runs(['failing', 'healthy', 'healthy', 'degraded']), 3);

    expect(strip.map((run) => run.outcome)).toEqual(['healthy', 'healthy', 'failing']);
  });
});

describe('availability trend', () => {
  it('steps from the oldest run to the latest and leaves a gap where nothing ran', () => {
    const results = [
      result({ outcome: 'healthy', started_at: NOW }),
      result({ outcome: 'failing', started_at: NOW - MINUTE }),
      // Nothing between these two and the oldest pair.
      result({ outcome: 'healthy', started_at: NOW - 9 * MINUTE }),
      result({ outcome: 'healthy', started_at: NOW - 10 * MINUTE }),
    ];

    expect(availabilityTrend(results, 5)).toEqual([1, null, null, null, 0.5]);
    expect(availabilityTrend([], 5)).toEqual([]);
  });

  it('colours a figure only when it is worth a second look', () => {
    expect(availabilityTone(undefined)).toBeUndefined();
    expect(availabilityTone(1)).toBeUndefined();
    expect(availabilityTone(0.99)).toBeUndefined();
    expect(availabilityTone(0.985)).toBe('warn');
    expect(availabilityTone(0.9)).toBe('danger');
  });
});

describe('checks needing attention', () => {
  it('lists the unwell checks worst first, then the longest unwell', () => {
    const rows = [
      checkRow({ monitor: { id: 'healthy', state: 'healthy' }, results: runs(['healthy']) }),
      checkRow({
        monitor: { id: 'degraded', state: 'degraded' },
        results: runs(['degraded', 'healthy'], { location_id: 'loc-eu' }),
      }),
      checkRow({
        monitor: { id: 'failing-new', state: 'failing' },
        results: runs(['failing', 'healthy']),
      }),
      checkRow({
        monitor: { id: 'failing-old', state: 'failing' },
        results: runs(['failing', 'failing', 'failing', 'healthy']),
      }),
      checkRow({
        monitor: { id: 'paused', state: 'failing', lifecycle: 'paused' },
        results: runs(['failing']),
      }),
    ];

    const attention = checksNeedingAttention(rows);

    expect(attention.map((item) => item.row.monitor.id)).toEqual([
      'failing-old',
      'failing-new',
      'degraded',
    ]);
    expect(attention[0]?.since).toBe(NOW - 3 * MINUTE);
    expect(attention[2]?.locationIds).toEqual(['loc-eu']);
  });
});
