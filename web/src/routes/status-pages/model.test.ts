import { describe, expect, it } from 'vitest';

import type { StatusPageIncident } from '@/api/statusPages';

import {
  affectedComponentNames,
  allowedNextStatuses,
  formatMicros,
  isIncidentWithinHistory,
  resolveStatusPageLanguage,
  statusPageHistoryDays,
  statusPageLanguages,
} from './model';

describe('status page model', () => {
  it('keeps resolved incidents terminal', () => {
    expect(allowedNextStatuses('resolved')).toEqual([]);
    expect(allowedNextStatuses('monitoring')).toContain('in_progress');
  });

  it('resolves affected component names without leaking missing ids', () => {
    const incident = {
      component_ids: ['api', 'removed'],
    } as StatusPageIncident;
    expect(affectedComponentNames(incident, [{ id: 'api', name: 'Public API' }])).toEqual([
      'Public API',
    ]);
  });

  it('formats persisted microseconds in the page timezone', () => {
    expect(formatMicros(0, 'UTC', 'en-us')).toContain('1970');
  });

  it('uses the selected default when the language collection is absent', () => {
    expect(statusPageLanguages('zh-cn')).toEqual(['zh-cn']);
    expect(statusPageLanguages('zh-cn', ['en-us', 'zh-cn'])).toEqual(['zh-cn', 'en-us']);
  });

  it('accepts only configured public-page language requests', () => {
    const languages = ['en-us', 'zh-cn'] as const;
    expect(resolveStatusPageLanguage('en-us', [...languages], 'zh-cn')).toBe('zh-cn');
    expect(resolveStatusPageLanguage('en-us', [...languages], 'fr-fr')).toBe('en-us');
  });

  it('defaults and bounds the configurable event history window', () => {
    expect(statusPageHistoryDays({})).toBe(90);
    expect(statusPageHistoryDays({ history_days: 30 })).toBe(30);
    expect(statusPageHistoryDays({ history_days: 0 })).toBe(90);
    expect(statusPageHistoryDays({ history_days: 366 })).toBe(90);
  });

  it('uses the resolved time when filtering historical incidents', () => {
    const generatedAt = Date.UTC(2026, 7, 10) * 1_000;
    expect(
      isIncidentWithinHistory(
        {
          started_at: Date.UTC(2026, 6, 1) * 1_000,
          ended_at: Date.UTC(2026, 7, 9) * 1_000,
        },
        generatedAt,
        7,
      ),
    ).toBe(true);
  });
});
