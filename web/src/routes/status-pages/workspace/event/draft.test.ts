import { describe, expect, it } from 'vitest';

import type { StatusPageIncident } from '@/api/statusPages';

import { eventInput, initialDraft, nextUpdateAt, promisedNextUpdate, promisesNextUpdate } from './draft';

const NOW = Date.UTC(2026, 9, 16, 8);

function incident(overrides: Partial<StatusPageIncident> = {}): StatusPageIncident {
  return {
    id: 'event',
    orgId: 'org',
    statusPageId: 'page',
    sourceIncidentId: null,
    kind: 'incident',
    title: 'API is slow',
    impact: 'major',
    status: 'investigating',
    publicationState: 'published',
    draftMessage: null,
    componentIds: ['api'],
    updates: [],
    startedAt: NOW * 1_000,
    endedAt: null,
    publishedAt: NOW * 1_000,
    createdAt: NOW * 1_000,
    updatedAt: NOW * 1_000,
    ...overrides,
  };
}

describe('event drafts', () => {
  it('promises a next update only while the event goes on', () => {
    expect(promisesNextUpdate('incident', 'identified')).toBe(true);
    expect(promisesNextUpdate('incident', 'resolved')).toBe(false);
    expect(promisesNextUpdate('maintenance', 'scheduled')).toBe(false);
    expect(promisesNextUpdate('maintenance', 'in_progress')).toBe(true);
    expect(promisesNextUpdate('maintenance', 'completed')).toBe(false);
  });

  it('turns a promise in minutes into the moment it points at', () => {
    expect(nextUpdateAt('incident', 'monitoring', 30, NOW)).toBe((NOW + 30 * 60_000) * 1_000);
    expect(nextUpdateAt('incident', 'monitoring', null, NOW)).toBeNull();
    expect(nextUpdateAt('incident', 'resolved', 30, NOW)).toBeNull();
  });

  it('decides who is told and what is promised only when the event goes out', () => {
    const draft = { ...initialDraft('incident', null), title: 'API', message: 'Looking', nextUpdateMinutes: 60, notify: false };

    expect(eventInput('incident', draft, 'draft', NOW)).not.toHaveProperty('notify');
    expect(eventInput('incident', draft, 'published', NOW)).toMatchObject({
      notify: false,
      nextUpdateAt: (NOW + 60 * 60_000) * 1_000,
    });
  });

  it('reads the promise of the latest update and none once the event is over', () => {
    const promised = (NOW + 15 * 60_000) * 1_000;
    const updates = [
      { id: 'b', orgId: 'org', statusPageId: 'page', incidentId: 'event', status: 'identified' as const, message: 'b', createdAt: 2, nextUpdateAt: promised },
      { id: 'a', orgId: 'org', statusPageId: 'page', incidentId: 'event', status: 'investigating' as const, message: 'a', createdAt: 1, nextUpdateAt: 99 },
    ];

    expect(promisedNextUpdate(incident({ updates }))).toBe(promised);
    expect(promisedNextUpdate(incident({ updates, status: 'resolved' }))).toBeNull();
  });
});
