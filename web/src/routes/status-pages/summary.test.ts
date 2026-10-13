import type { TFunction } from 'i18next';
import { describe, expect, it } from 'vitest';

import type {
  StatusPageComponent,
  StatusPageIncident,
  StatusPageSnapshot,
} from '@/api/statusPages';

import {
  componentSpectrum,
  eventPath,
  eventsUnderWay,
  latestUpdate,
  maintenanceAhead,
  seenComponents,
  worstStatus,
} from './summary';

const now = Date.UTC(2026, 7, 10, 12) * 1_000;
const HOUR = 60 * 60 * 1_000_000;

function component(id: string, overrides: Partial<StatusPageComponent> = {}): StatusPageComponent {
  return {
    id,
    org_id: 'org',
    status_page_id: 'page',
    name: id,
    description: '',
    status: 'operational',
    visibility: 'enabled',
    lifecycle: 'active',
    position: 0,
    archived_at: null,
    created_at: now,
    updated_at: now,
    ...overrides,
  };
}

function event(id: string, overrides: Partial<StatusPageIncident> = {}): StatusPageIncident {
  return {
    id,
    org_id: 'org',
    status_page_id: 'page',
    source_incident_id: null,
    kind: 'incident',
    title: id,
    impact: 'minor',
    status: 'investigating',
    publication_state: 'published',
    draft_message: null,
    component_ids: [],
    updates: [],
    started_at: now - HOUR,
    ended_at: null,
    published_at: now - HOUR,
    created_at: now - HOUR,
    updated_at: now - HOUR,
    ...overrides,
  };
}

function snapshot(overrides: Partial<StatusPageSnapshot> = {}): StatusPageSnapshot {
  return {
    page: {} as StatusPageSnapshot['page'],
    overall_status: 'operational',
    components: [],
    component_status_events: [],
    active_incidents: [],
    scheduled_maintenance: [],
    history: [],
    updated_at: now,
    generated_at: now,
    ...overrides,
  };
}

const t = ((key: string) => key) as unknown as TFunction;

describe('status page summary', () => {
  it('ranks statuses as the server does: maintenance is the mildest thing that is not well', () => {
    expect(worstStatus('operational', 'maintenance')).toBe('maintenance');
    expect(worstStatus('maintenance', 'degraded_performance')).toBe('degraded_performance');
    expect(worstStatus('major_outage', 'partial_outage')).toBe('major_outage');
  });

  it('shows a component in the worse of its own status and what is under way on it', () => {
    const seen = seenComponents(
      snapshot({
        components: [
          component('api', { position: 1 }),
          component('billing', { position: 2, status: 'degraded_performance' }),
          component('search', { position: 3 }),
        ],
        active_incidents: [
          event('api-errors', { impact: 'major', component_ids: ['api', 'billing'] }),
          event('api-upgrade', { kind: 'maintenance', impact: 'maintenance', component_ids: ['api'] }),
        ],
      }),
    );

    expect(seen.map(({ component: { id }, status }) => [id, status])).toEqual([
      ['api', 'partial_outage'],
      ['billing', 'partial_outage'],
      ['search', 'operational'],
    ]);
  });

  it('leaves out what the public page does not list, and keeps its order', () => {
    const seen = seenComponents(
      snapshot({
        components: [
          component('last', { position: 9 }),
          component('hidden', { position: 1, visibility: 'hidden' }),
          component('archived', { position: 2, lifecycle: 'archived' }),
          component('first', { position: 3 }),
        ],
      }),
    );

    expect(seen.map(({ component: { id } }) => id)).toEqual(['first', 'last']);
  });

  it('divides the components by status, keeping the statuses nothing is in', () => {
    const spectrum = componentSpectrum(t, [
      { component: component('a'), status: 'operational' },
      { component: component('b'), status: 'operational' },
      { component: component('c'), status: 'major_outage' },
    ]);

    expect(spectrum.map(({ key, value }) => [key, value])).toEqual([
      ['operational', 2],
      ['degraded_performance', 0],
      ['partial_outage', 0],
      ['major_outage', 1],
      ['maintenance', 0],
    ]);
    expect(spectrum[3]).toMatchObject({ label: 'component_status.major_outage', color: 'var(--red)' });
  });

  it('puts the worst of what is under way first, and announced maintenance soonest first', () => {
    const page = snapshot({
      active_incidents: [
        event('maintenance', { kind: 'maintenance', impact: 'maintenance' }),
        event('minor', { impact: 'minor' }),
        event('critical', { impact: 'critical', started_at: now - 5 * HOUR }),
      ],
      scheduled_maintenance: [
        event('later', { kind: 'maintenance', started_at: now + 48 * HOUR }),
        event('sooner', { kind: 'maintenance', started_at: now + HOUR }),
      ],
    });

    expect(eventsUnderWay(page).map(({ id }) => id)).toEqual(['critical', 'minor', 'maintenance']);
    expect(maintenanceAhead(page).map(({ id }) => id)).toEqual(['sooner', 'later']);
    expect(page.active_incidents[0]?.id).toBe('maintenance');
  });

  it('finds the latest thing said about an event', () => {
    const update = (id: string, at: number) => ({
      id,
      org_id: 'org',
      status_page_id: 'page',
      incident_id: 'one',
      status: 'investigating' as const,
      message: id,
      created_at: at,
    });

    expect(latestUpdate(event('quiet'))).toBeUndefined();
    expect(
      latestUpdate(event('one', { updates: [update('first', now - 2 * HOUR), update('last', now), update('middle', now - HOUR)] }))?.id,
    ).toBe('last');
  });

  it('sends an event to the tab it is managed under', () => {
    expect(eventPath('page', event('one'))).toBe('/status-pages/page/incidents/one');
    expect(eventPath('page', event('two', { kind: 'maintenance' }))).toBe('/status-pages/page/maintenance/two');
  });
});
