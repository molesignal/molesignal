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
    orgId: 'org',
    statusPageId: 'page',
    name: id,
    description: '',
    status: 'operational',
    visibility: 'enabled',
    lifecycle: 'active',
    position: 0,
    archivedAt: null,
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

function event(id: string, overrides: Partial<StatusPageIncident> = {}): StatusPageIncident {
  return {
    id,
    orgId: 'org',
    statusPageId: 'page',
    sourceIncidentId: null,
    kind: 'incident',
    title: id,
    impact: 'minor',
    status: 'investigating',
    publicationState: 'published',
    draftMessage: null,
    componentIds: [],
    updates: [],
    startedAt: now - HOUR,
    endedAt: null,
    publishedAt: now - HOUR,
    createdAt: now - HOUR,
    updatedAt: now - HOUR,
    ...overrides,
  };
}

function snapshot(overrides: Partial<StatusPageSnapshot> = {}): StatusPageSnapshot {
  return {
    page: {} as StatusPageSnapshot['page'],
    overallStatus: 'operational',
    components: [],
    componentStatusEvents: [],
    activeIncidents: [],
    scheduledMaintenance: [],
    history: [],
    updatedAt: now,
    generatedAt: now,
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
        activeIncidents: [
          event('api-errors', { impact: 'major', componentIds: ['api', 'billing'] }),
          event('api-upgrade', { kind: 'maintenance', impact: 'maintenance', componentIds: ['api'] }),
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
      activeIncidents: [
        event('maintenance', { kind: 'maintenance', impact: 'maintenance' }),
        event('minor', { impact: 'minor' }),
        event('critical', { impact: 'critical', startedAt: now - 5 * HOUR }),
      ],
      scheduledMaintenance: [
        event('later', { kind: 'maintenance', startedAt: now + 48 * HOUR }),
        event('sooner', { kind: 'maintenance', startedAt: now + HOUR }),
      ],
    });

    expect(eventsUnderWay(page).map(({ id }) => id)).toEqual(['critical', 'minor', 'maintenance']);
    expect(maintenanceAhead(page).map(({ id }) => id)).toEqual(['sooner', 'later']);
    expect(page.activeIncidents[0]?.id).toBe('maintenance');
  });

  it('finds the latest thing said about an event', () => {
    const update = (id: string, at: number) => ({
      id,
      orgId: 'org',
      statusPageId: 'page',
      incidentId: 'one',
      status: 'investigating' as const,
      message: id,
      createdAt: at,
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
