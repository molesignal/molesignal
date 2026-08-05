import { describe, expect, it } from 'vitest';

import type { ReplayEvent, SessionEvent } from '@/api/rum';

import { normalizePlayerEvents } from './events';

describe('normalizePlayerEvents', () => {
  it('uses recorded timestamps instead of a fixed playback interval', () => {
    const replay: ReplayEvent[] = [
      { type: 'view', ts: 1_000, name: 'Home' },
      { type: 'click', ts: 4_750, name: 'Checkout' },
    ];

    expect(normalizePlayerEvents(replay, []).map((event) => event.timestamp)).toEqual([
      1_000,
      4_750,
    ]);
  });

  it('keeps action-only events while removing correlated duplicates', () => {
    const replay: ReplayEvent[] = [
      { type: 'click', ts: 2_000, name: 'Buy' },
    ];
    const actions: SessionEvent[] = [
      { type: 'click', ts_micros: 2_000_000, name: 'Buy', payload: {} },
      { type: 'error', ts_micros: 2_500_000, name: 'Payment failed', payload: {} },
    ];

    expect(normalizePlayerEvents(replay, actions).map((event) => event.label)).toEqual([
      'Buy',
      'Payment failed',
    ]);
  });
});
