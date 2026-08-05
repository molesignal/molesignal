import { describe, expect, it } from 'vitest';

import { computeRedRing, RED_RING_ENTER, RED_RING_EXIT } from '@/stores/useTopologyFlags';
import { serviceHealthStatus } from '@/viz/topology/ServiceNode';

describe('ServiceNode health border', () => {
  describe('warning-border hysteresis', () => {
    it('turns on at the enter threshold (0.05)', () => {
      expect(computeRedRing(false, 0.04)).toBe(false);
      expect(computeRedRing(false, RED_RING_ENTER)).toBe(true);
      expect(computeRedRing(false, 0.10)).toBe(true);
    });

    it('stays on between 0.045 and 0.05 once tripped', () => {
      // Already on at 0.05; oscillating 0.049 ↔ 0.051 keeps it on.
      expect(computeRedRing(true, 0.049)).toBe(true);
      expect(computeRedRing(true, 0.051)).toBe(true);
      // The boundary itself stays on (>= RED_RING_EXIT).
      expect(computeRedRing(true, RED_RING_EXIT)).toBe(true);
    });

    it('turns off only when err_rate drops below the exit threshold (0.045)', () => {
      expect(computeRedRing(true, 0.044)).toBe(false);
      expect(computeRedRing(true, 0)).toBe(false);
    });

    it('does not flicker over an oscillating sequence', () => {
      // Simulate poll-to-poll error rates that straddle the boundary; the
      // ring should latch on at the first cross above ENTER and stay on
      // through every drop that remains >= EXIT.
      let on = false;
      const seq = [0.04, 0.049, 0.051, 0.049, 0.051, 0.047, 0.046, 0.044];
      const states = seq.map((r) => {
        on = computeRedRing(on, r);
        return on;
      });
      expect(states).toEqual([false, false, true, true, true, true, true, false]);
    });
  });

  it('maps error-rate bands to the four service-graph border colors', () => {
    expect(serviceHealthStatus(0.009)).toBe('healthy');
    expect(serviceHealthStatus(0.01)).toBe('degraded');
    expect(serviceHealthStatus(0.05)).toBe('warning');
    expect(serviceHealthStatus(0.1)).toBe('critical');
  });
});
