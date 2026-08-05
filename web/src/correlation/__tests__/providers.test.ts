import { describe, expect, it } from 'vitest';

import { PROVIDERS, providersFor } from '@/correlation/providers';

describe('correlation providers', () => {
  it('exposes 8 providers covering m/t/l/h/s domains', () => {
    expect(PROVIDERS.length).toBeGreaterThanOrEqual(8);
  });

  it('providersFor returns providers whose `from` matches', () => {
    const fromMetric = providersFor('metric');
    expect(fromMetric.every((p) => p.from === 'metric')).toBe(true);
    expect(fromMetric.length).toBeGreaterThan(0);
  });

  it('each provider derive returns a defined CorrelationContext given a minimal ctx', () => {
    for (const p of PROVIDERS) {
      const out = p.derive({
        kind: p.from,
        globalWindow: { from: 'now-1h', to: 'now', mode: 'relative' },
        fields: {},
      });
      expect(out).toBeTruthy();
    }
  });
});
