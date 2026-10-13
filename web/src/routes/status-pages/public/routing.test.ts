import { describe, expect, it } from 'vitest';

import type {
  PublicStatusPageIncident,
  PublicStatusPageSnapshot,
} from '@/api/statusPages';

import {
  findPublicIncident,
  latestFirst,
  ongoingPublicIncidents,
  pastIncidentDays,
  pastPublicIncidents,
  publicIncidentPath,
  publicStatusPaths,
  upcomingMaintenance,
} from './routing';

const HOUR = 60 * 60 * 1_000_000;
const DAY = 24 * HOUR;
const now = Date.UTC(2026, 7, 10, 12) * 1_000;

function incident(id: string, overrides: Partial<PublicStatusPageIncident> = {}): PublicStatusPageIncident {
  return {
    id,
    kind: 'incident',
    title: id,
    impact: 'minor',
    status: 'investigating',
    component_ids: [],
    updates: [],
    started_at: now - HOUR,
    ended_at: null,
    ...overrides,
  };
}

function snapshot(overrides: Partial<PublicStatusPageSnapshot> = {}): PublicStatusPageSnapshot {
  return {
    page: {
      name: 'Acme',
      slug: 'acme',
      logo_url: null,
      brand_color: '#4F46E5',
      timezone: 'UTC',
      language: 'en-us',
      languages: ['en-us'],
      history_days: 90,
      visibility: 'public',
    },
    overall_status: 'operational',
    components: [],
    component_status_events: [],
    active_incidents: [],
    scheduled_maintenance: [],
    history: [],
    generated_at: now,
    ...overrides,
  };
}

describe('public status routing', () => {
  it('keeps every link on the root of a custom domain, and under the slug elsewhere', () => {
    expect(publicStatusPaths('domain', 'acme', 'zh-cn')).toEqual({
      current: '/?lang=zh-cn',
      history: '/history?lang=zh-cn',
      uptime: '/uptime?lang=zh-cn',
    });
    expect(publicStatusPaths('slug', 'acme cloud', 'en-us').history).toBe(
      '/status/acme%20cloud/history?lang=en-us',
    );
    expect(publicIncidentPath('domain', 'acme', 'a/b')).toBe('/incidents/a%2Fb');
    expect(publicIncidentPath('slug', 'acme', 'one')).toBe('/status/acme/incidents/one');
  });

  it('finds an incident wherever the snapshot keeps it', () => {
    const page = snapshot({
      active_incidents: [incident('going-on')],
      scheduled_maintenance: [incident('planned', { kind: 'maintenance', status: 'scheduled' })],
      history: [incident('over', { status: 'resolved', ended_at: now - DAY })],
    });

    expect(findPublicIncident(page, 'going-on')?.id).toBe('going-on');
    expect(findPublicIncident(page, 'planned')?.id).toBe('planned');
    expect(findPublicIncident(page, 'over')?.id).toBe('over');
    expect(findPublicIncident(page, 'nowhere')).toBeUndefined();
  });

  it('puts the worst of what is going on first, then the one that began last', () => {
    const page = snapshot({
      active_incidents: [
        incident('maintenance', { kind: 'maintenance', impact: 'maintenance', status: 'in_progress' }),
        incident('minor-old', { impact: 'minor', started_at: now - 3 * HOUR }),
        incident('critical', { impact: 'critical', started_at: now - 5 * HOUR }),
        incident('minor-new', { impact: 'minor', started_at: now - HOUR }),
        incident('major', { impact: 'major' }),
      ],
    });

    expect(ongoingPublicIncidents(page).map(({ id }) => id)).toEqual([
      'critical',
      'major',
      'minor-new',
      'minor-old',
      'maintenance',
    ]);
    // The snapshot is somebody else's; it is not sorted in place.
    expect(page.active_incidents[0]?.id).toBe('maintenance');
  });

  it('lists announced maintenance by how soon it begins', () => {
    const page = snapshot({
      scheduled_maintenance: [
        incident('next-week', { kind: 'maintenance', status: 'scheduled', started_at: now + 7 * DAY }),
        incident('tomorrow', { kind: 'maintenance', status: 'scheduled', started_at: now + DAY }),
      ],
    });

    expect(upcomingMaintenance(page).map(({ id }) => id)).toEqual(['tomorrow', 'next-week']);
  });

  it('looks back a fortnight, or as far as the page keeps if that is less', () => {
    expect(pastIncidentDays(snapshot())).toBe(14);
    expect(pastIncidentDays(snapshot({ page: { ...snapshot().page, history_days: 7 } }))).toBe(7);
  });

  it('lists what ended within those days, the latest first, and nothing that is still open', () => {
    const page = snapshot({
      active_incidents: [incident('going-on')],
      history: [
        incident('a-week-ago', { status: 'resolved', ended_at: now - 7 * DAY }),
        incident('yesterday', { status: 'resolved', ended_at: now - DAY }),
        incident('last-month', { status: 'resolved', ended_at: now - 30 * DAY }),
        // A snapshot that lists something both as open and as over: open wins.
        incident('going-on', { status: 'resolved', ended_at: now - HOUR }),
      ],
    });

    expect(pastPublicIncidents(page).map(({ id }) => id)).toEqual(['yesterday', 'a-week-ago']);
  });

  it('reads the updates of an incident from the latest back', () => {
    const subject = incident('one', {
      updates: [
        { id: 'first', status: 'investigating', message: '', created_at: now - 3 * HOUR },
        { id: 'third', status: 'resolved', message: '', created_at: now - HOUR },
        { id: 'second', status: 'identified', message: '', created_at: now - 2 * HOUR },
      ],
    });

    expect(latestFirst(subject).map(({ id }) => id)).toEqual(['third', 'second', 'first']);
    expect(subject.updates[0]?.id).toBe('first');
  });
});
