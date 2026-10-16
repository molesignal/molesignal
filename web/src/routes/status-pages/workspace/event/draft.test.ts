import { describe, expect, it } from 'vitest';

import type { StatusPageIncident } from '@/api/statusPages';

import { eventInput, initialDraft, nextUpdateAt, promisedNextUpdate, promisesNextUpdate } from './draft';

const NOW = Date.UTC(2026, 9, 16, 8);

function incident(overrides: Partial<StatusPageIncident> = {}): StatusPageIncident {
  return {
    id: 'event',
    org_id: 'org',
    status_page_id: 'page',
    source_incident_id: null,
    kind: 'incident',
    title: 'API is slow',
    impact: 'major',
    status: 'investigating',
    publication_state: 'published',
    draft_message: null,
    component_ids: ['api'],
    updates: [],
    started_at: NOW * 1_000,
    ended_at: null,
    published_at: NOW * 1_000,
    created_at: NOW * 1_000,
    updated_at: NOW * 1_000,
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
      next_update_at: (NOW + 60 * 60_000) * 1_000,
    });
  });

  it('reads the promise of the latest update and none once the event is over', () => {
    const promised = (NOW + 15 * 60_000) * 1_000;
    const updates = [
      { id: 'b', org_id: 'org', status_page_id: 'page', incident_id: 'event', status: 'identified' as const, message: 'b', created_at: 2, next_update_at: promised },
      { id: 'a', org_id: 'org', status_page_id: 'page', incident_id: 'event', status: 'investigating' as const, message: 'a', created_at: 1, next_update_at: 99 },
    ];

    expect(promisedNextUpdate(incident({ updates }))).toBe(promised);
    expect(promisedNextUpdate(incident({ updates, status: 'resolved' }))).toBeNull();
  });
});
