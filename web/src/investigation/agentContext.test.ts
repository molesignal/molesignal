import { describe, expect, it } from 'vitest';

import {
  chatContextFromFilters,
  contextStreamHints,
  timeRangeMicros,
  timeWindowForPreset,
} from './agentContext';

describe('Mole Agent investigation context', () => {
  it('derives visible context and backend hints from pinned filters', () => {
    const filters = [
      { key: 'service', value: 'checkout', operator: '=' as const },
      { key: 'environment', value: 'production', operator: '=' as const },
      { key: 'host', value: 'node-3', operator: '!=' as const },
    ];
    const context = chatContextFromFilters(filters);

    expect(context).toEqual({
      service: 'checkout',
      environment: 'production',
      alert: '',
    });
    expect(contextStreamHints(context, filters)).toEqual([
      'environment:production',
      'service:checkout',
      'host!=node-3',
    ]);
  });

  it('uses the same global time window sent to the backend', () => {
    const window = timeWindowForPreset('6h');
    expect(window).not.toBeNull();
    const range = timeRangeMicros(window!);
    expect(range.end_micros - range.start_micros).toBe(6 * 3_600 * 1_000_000);
  });
});
