import { describe, expect, it } from 'vitest';

import {
  matchesStreamHealthFilter,
  streamAttentionHref,
  summarizeStreamHealth,
} from './streamHealth';

describe('unified stream health', () => {
  it('uses one attention taxonomy for Home and Streams', () => {
    const summary = summarizeStreamHealth([
      { status: 'healthy' },
      { status: 'idle' },
      { status: 'delayed' },
      { status: 'interrupted' },
    ]);
    expect(summary).toEqual({
      total: 4,
      receiving: 1,
      attention: 2,
      inactive: 1,
      status: 'degraded',
    });
    expect(matchesStreamHealthFilter('delayed', 'attention')).toBe(true);
    expect(matchesStreamHealthFilter('idle', 'attention')).toBe(false);
  });

  it('creates a recoverable attention CTA with filter and time', () => {
    const url = new URL(streamAttentionHref(24 * 60 * 60), 'https://molesignal.local');
    expect(url.pathname).toBe('/streams');
    expect(url.searchParams.get('status')).toBe('attention');
    expect(url.searchParams.get('window_secs')).toBe('86400');
    expect(url.searchParams.get('time')).toBe('now-1d..now');
  });
});
