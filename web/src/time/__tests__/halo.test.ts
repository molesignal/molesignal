import { describe, expect, it } from 'vitest';

import { halo } from '@/time/halo';

describe('halo intersection', () => {
  const global = {
    from: '2026-05-23T09:00:00.000Z',
    to: '2026-05-23T11:00:00.000Z',
    mode: 'absolute' as const,
  };

  it('builds ±30s window for trace_span around the anchor', () => {
    const w = halo('trace_span', '2026-05-23T10:00:00.000Z', global);
    expect(w.mode).toBe('absolute');
    expect(Date.parse(w.to) - Date.parse(w.from)).toBe(60_000);
  });

  it('clamps to global window when halo extends beyond it', () => {
    // anchor is at global.from — halo would extend 5s before; expect clamp to from
    const w = halo('log_row', global.from, global);
    expect(Date.parse(w.from)).toBe(Date.parse(global.from));
  });

  it('uses ±60s for metric_sample, ±5s for log_row, ±30s for trace_span', () => {
    const anchor = '2026-05-23T10:00:00.000Z';
    expect(Date.parse(halo('metric_sample', anchor, global).to) - Date.parse(anchor)).toBe(60_000);
    expect(Date.parse(halo('log_row', anchor, global).to) - Date.parse(anchor)).toBe(5_000);
    expect(Date.parse(halo('trace_span', anchor, global).to) - Date.parse(anchor)).toBe(30_000);
  });
});
