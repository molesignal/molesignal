import { describe, expect, it } from 'vitest';

import type { BusinessService, BusinessServiceStatus } from '@/api/businessServices';

import { operationalShare, sortServices, timelineSegments, TIMELINE_MICROS } from './model';

const HOUR = 60 * 60 * 1_000_000;

function service(name: string, status: BusinessServiceStatus): BusinessService {
  return {
    id: name,
    org_id: 'org',
    name,
    description: '',
    dependencies: [],
    health: { status, causes: [], changed_at: null, evaluated_at: null, history: [] },
    created_by: null,
    created_at: 0,
    updated_at: 0,
    linked_components: [],
  };
}

describe('business service model', () => {
  it('puts the worst health first, then sorts by name', () => {
    const sorted = sortServices([
      service('Search', 'operational'),
      service('Checkout', 'major_outage'),
      service('Billing', 'operational'),
      service('Payments', 'degraded_performance'),
    ]);

    expect(sorted.map((item) => item.name)).toEqual(['Checkout', 'Payments', 'Billing', 'Search']);
  });

  it('lays the last seven days out as shares of the bar and leaves time before the first evaluation blank', () => {
    const now = 10 * TIMELINE_MICROS;
    const segments = timelineSegments(
      [
        // Began before the window: only the part inside it counts.
        { status: 'operational', started_at: now - TIMELINE_MICROS - HOUR, ended_at: now - 24 * HOUR },
        { status: 'major_outage', started_at: now - 24 * HOUR, ended_at: now - 12 * HOUR },
        { status: 'operational', started_at: now - 12 * HOUR, ended_at: null },
      ],
      now,
    );

    expect(segments.map((segment) => segment.status)).toEqual(['operational', 'major_outage', 'operational']);
    expect(segments[0]?.offset).toBe(0);
    expect(segments[1]?.width).toBeCloseTo(12 / (7 * 24));
    expect(segments.at(-1)?.endedAt).toBe(now);
    expect(operationalShare(segments)).toBeCloseTo(1 - 12 / (7 * 24));
  });

  it('says nothing about uptime before the first evaluation', () => {
    expect(operationalShare(timelineSegments([], 1_000))).toBeNull();
  });
});
