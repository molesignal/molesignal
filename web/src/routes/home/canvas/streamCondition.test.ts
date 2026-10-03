import { describe, expect, it } from 'vitest';

import {
  classifyStreamCondition,
  STALE_AFTER_MICROS,
  summarizeFleet,
} from './streamCondition';

const NOW = 1_800_000_000_000_000;
const HOUR = 3_600_000_000;

describe('classifyStreamCondition', () => {
  it('keeps a recent interruption urgent and calls a long silence stale', () => {
    expect(
      classifyStreamCondition({
        status: 'interrupted',
        lastReceivedAtMicros: NOW - 2 * HOUR,
        nowMicros: NOW,
      }),
    ).toBe('interrupted');
    expect(
      classifyStreamCondition({
        status: 'interrupted',
        lastReceivedAtMicros: NOW - STALE_AFTER_MICROS - HOUR,
        nowMicros: NOW,
      }),
    ).toBe('stale');
  });

  it('treats an interruption with no known last receive as urgent', () => {
    expect(
      classifyStreamCondition({
        status: 'interrupted',
        lastReceivedAtMicros: null,
        nowMicros: NOW,
      }),
    ).toBe('interrupted');
  });

  it('reads second-resolution timestamps from older payloads', () => {
    expect(
      classifyStreamCondition({
        status: 'interrupted',
        lastReceivedAtMicros: NOW / 1_000_000 - 3 * 24 * 3600,
        nowMicros: NOW,
      }),
    ).toBe('stale');
  });

  it('maps the remaining runtime statuses', () => {
    const at = (status: Parameters<typeof classifyStreamCondition>[0]['status']) =>
      classifyStreamCondition({ status, lastReceivedAtMicros: NOW, nowMicros: NOW });
    expect(at('healthy')).toBe('healthy');
    expect(at('delayed')).toBe('delayed');
    expect(at('idle')).toBe('inactive');
    expect(at('unused')).toBe('inactive');
    expect(at('unknown')).toBe('unknown');
  });
});

describe('summarizeFleet', () => {
  const stream = (
    status: Parameters<typeof classifyStreamCondition>[0]['status'],
    lastReceivedAgoMicros: number | null,
  ) => ({
    status,
    last_received_at_micros:
      lastReceivedAgoMicros == null ? null : NOW - lastReceivedAgoMicros,
  });

  it('has no verdict condition for an empty fleet', () => {
    expect(summarizeFleet([], NOW)).toMatchObject({
      total: 0,
      verdict: 'no_streams',
      affected: 0,
      uniform: false,
      lastReceivedAtMicros: null,
    });
  });

  it('collapses a fleet that has been silent for days into one stale verdict', () => {
    const fleet = summarizeFleet(
      Array.from({ length: 15 }, () => stream('interrupted', 6 * 24 * HOUR)),
      NOW,
    );
    expect(fleet).toMatchObject({
      total: 15,
      verdict: 'stale',
      affected: 15,
      receiving: 0,
      uniform: true,
    });
    expect(fleet.lastReceivedAtMicros).toBe(NOW - 6 * 24 * HOUR);
  });

  it('ranks a recent interruption above delay, staleness and health', () => {
    const fleet = summarizeFleet(
      [
        stream('healthy', 1_000_000),
        stream('delayed', 10 * 60 * 1_000_000),
        stream('interrupted', 5 * 24 * HOUR),
        stream('interrupted', HOUR),
      ],
      NOW,
    );
    expect(fleet.verdict).toBe('interrupted');
    expect(fleet.affected).toBe(1);
    expect(fleet.receiving).toBe(1);
    expect(fleet.uniform).toBe(false);
    expect(fleet.counts).toMatchObject({
      healthy: 1,
      delayed: 1,
      stale: 1,
      interrupted: 1,
    });
  });

  it('reports healthy when something receives and nothing needs attention', () => {
    const fleet = summarizeFleet(
      [stream('healthy', 1_000_000), stream('idle', 3 * 24 * HOUR)],
      NOW,
    );
    expect(fleet.verdict).toBe('healthy');
    expect(fleet.uniform).toBe(false);
  });

  it('reports no_data when every stream is idle or unused', () => {
    const fleet = summarizeFleet(
      [stream('idle', null), stream('unused', null)],
      NOW,
    );
    expect(fleet).toMatchObject({
      verdict: 'no_data',
      affected: 2,
      uniform: true,
      lastReceivedAtMicros: null,
    });
  });
});
