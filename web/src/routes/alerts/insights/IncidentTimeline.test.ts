import { describe, expect, it } from 'vitest';

import { buildIncidentTimeline } from './IncidentTimeline';

describe('buildIncidentTimeline', () => {
  it('keeps the actual date on rolling hourly buckets', () => {
    const now = new Date(2026, 7, 15, 10, 30).getTime();
    const createdAt = new Date(2026, 7, 14, 12, 15).getTime();
    const buckets = buildIncidentTimeline(
      [{ created_at: createdAt * 1_000 }],
      86_400,
      now,
      'zh-CN',
    );

    const bucket = buckets.find((item) => item.count === 1);
    expect(buckets).toHaveLength(1);
    expect(bucket?.label).toBe('8月14日 12:00');
    expect(bucket?.tooltipLabel).toContain('2026');
    expect(bucket?.startMs).toBe(new Date(2026, 7, 14, 12).getTime());
  });

  it('uses local calendar days for longer windows', () => {
    const now = new Date(2026, 7, 15, 10, 30).getTime();
    const createdAt = new Date(2026, 7, 13, 23, 45).getTime();
    const buckets = buildIncidentTimeline(
      [{ created_at: createdAt * 1_000 }],
      604_800,
      now,
      'zh-CN',
    );

    const bucket = buckets.find((item) => item.count === 1);
    expect(buckets).toHaveLength(1);
    expect(bucket?.label).toBe('8月13日');
    expect(bucket?.startMs).toBe(new Date(2026, 7, 13).getTime());
    expect(bucket?.tooltipLabel).toContain('2026');
  });

  it('ignores active incidents created before the selected range', () => {
    const now = new Date(2026, 7, 15, 10, 30).getTime();
    const oldIncident = new Date(2026, 6, 1).getTime();
    const buckets = buildIncidentTimeline(
      [{ created_at: oldIncident * 1_000 }],
      604_800,
      now,
      'zh-CN',
    );

    expect(buckets).toEqual([]);
  });
});
