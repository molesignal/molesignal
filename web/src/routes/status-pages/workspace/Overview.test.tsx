import { cleanup, render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type {
  StatusPageComponent,
  StatusPageIncident,
  StatusPageSnapshot,
} from '@/api/statusPages';
import i18n from '@/i18n';

import { useStatusPageWorkspace } from './Layout';
import { StatusPageOverview } from './Overview';

vi.mock('./Layout', () => ({
  useStatusPageWorkspace: vi.fn(),
}));

const workspace = vi.mocked(useStatusPageWorkspace);
const now = Date.UTC(2026, 7, 10, 12) * 1_000;
const MINUTE = 60 * 1_000_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

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
    impact: 'major',
    status: 'investigating',
    publication_state: 'published',
    draft_message: null,
    component_ids: ['API'],
    updates: [],
    started_at: now - 38 * MINUTE,
    ended_at: null,
    published_at: now - 38 * MINUTE,
    created_at: now - 38 * MINUTE,
    updated_at: now - 12 * MINUTE,
    ...overrides,
  };
}

function renderOverview(snapshot: Partial<StatusPageSnapshot>, allowed = true) {
  workspace.mockReturnValue({
    pageId: 'page',
    snapshot: {
      page: { lifecycle: 'active' },
      overall_status: 'operational',
      components: [component('API'), component('Dashboard', { position: 1 }), component('Billing', { position: 2 })],
      component_status_events: [],
      active_incidents: [],
      scheduled_maintenance: [],
      history: [],
      updated_at: now - 12 * MINUTE,
      generated_at: now,
      ...snapshot,
    },
    manageAccess: { allowed, disabled: !allowed, reason: allowed ? undefined : 'No access' },
  } as unknown as ReturnType<typeof useStatusPageWorkspace>);

  return render(
    <MemoryRouter>
      <StatusPageOverview />
    </MemoryRouter>,
  );
}

describe('StatusPageOverview', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
  });

  afterEach(() => {
    workspace.mockReset();
    cleanup();
  });

  it('opens with what customers see: the verdict, what it rests on, how the components divide', () => {
    const { container } = renderOverview({
      overall_status: 'partial_outage',
      active_incidents: [
        event('API errors', {
          updates: [
            { id: 'u1', org_id: 'org', status_page_id: 'page', incident_id: 'API errors', status: 'investigating', message: 'Looking into it.', created_at: now - 38 * MINUTE },
            { id: 'u2', org_id: 'org', status_page_id: 'page', incident_id: 'API errors', status: 'identified', message: 'Rolling the change back.', created_at: now - 12 * MINUTE },
          ],
        }),
      ],
      scheduled_maintenance: [
        event('Export move', { kind: 'maintenance', impact: 'maintenance', status: 'scheduled', component_ids: ['Billing'], started_at: now + 2 * DAY }),
      ],
    });

    const standing = container.querySelector('[data-status-page-standing]') as HTMLElement;
    expect(standing.getAttribute('data-status-page-standing')).toBe('partial_outage');
    expect(within(standing).getByRole('heading', { name: 'Partial outage' })).toBeTruthy();
    expect(standing.textContent).toContain('1 event under way · 1 maintenance scheduled');
    expect(standing.textContent).toContain('last change 12 minutes ago');
    // The incident puts its component in the state the public page shows it in.
    expect(within(standing).getByRole('img', { name: /Operational 2, .*Partial outage 1/ })).toBeTruthy();

    const underWay = screen.getByRole('heading', { name: 'Under way' }).closest('section') as HTMLElement;
    const row = within(underWay).getByRole('link', { name: /API errors/ });
    expect(row.getAttribute('href')).toBe('/status-pages/page/incidents/API%20errors');
    expect(row.textContent).toContain('started 38 minutes ago');
    // Only the latest thing said about it.
    expect(row.textContent).toContain('Rolling the change back.');
    expect(row.textContent).not.toContain('Looking into it.');

    const planned = screen.getByRole('heading', { name: 'Scheduled maintenance' }).closest('section') as HTMLElement;
    expect(within(planned).getByRole('link', { name: /Export move/ }).textContent).toContain('in 2 days');

    const unwell = screen.getByRole('heading', { name: 'Not operational' }).closest('section') as HTMLElement;
    expect(within(unwell).getByRole('link', { name: /API/ }).textContent).toContain('Partial outage');
    expect(within(unwell).queryByText('Dashboard')).toBeNull();
  });

  it('sends maintenance that is under way to the maintenance tab, not to the incidents', () => {
    renderOverview({
      overall_status: 'maintenance',
      active_incidents: [
        event('Database upgrade', { kind: 'maintenance', impact: 'maintenance', status: 'in_progress' }),
      ],
    });

    const row = screen.getByRole('link', { name: /Database upgrade/ });
    expect(row.getAttribute('href')).toBe('/status-pages/page/maintenance/Database%20upgrade');
    // Planned work is "in progress"; only an incident has a "fix in progress".
    expect(within(row).getByText('In progress')).toBeTruthy();
  });

  it('draws no empty parts when nothing is going on', () => {
    const { container } = renderOverview({});

    const standing = container.querySelector('[data-status-page-standing]') as HTMLElement;
    expect(within(standing).getByRole('heading', { name: 'Operational' })).toBeTruthy();
    expect(standing.textContent).toContain('Nothing is under way and nothing is planned');
    // The statement has said it; there are no cards under it that say "nothing" again.
    expect(screen.queryByRole('heading', { name: 'Under way' })).toBeNull();
    expect(screen.queryByRole('heading', { name: 'Scheduled maintenance' })).toBeNull();
    expect(screen.queryByRole('heading', { name: 'Not operational' })).toBeNull();
    expect(screen.queryByRole('heading', { name: 'Lately' })).toBeNull();
    expect(container.querySelectorAll('[data-status-page-section]')).toHaveLength(0);
  });

  it('recalls the last few things that ended, and where each is kept', () => {
    renderOverview({
      history: Array.from({ length: 6 }, (_, index) =>
        event(`Ended ${index + 1}`, {
          status: 'resolved',
          started_at: now - (index + 1) * DAY - 2 * HOUR,
          ended_at: now - (index + 1) * DAY,
        }),
      ),
    });

    const lately = screen.getByRole('heading', { name: 'Lately' }).closest('section') as HTMLElement;
    const rows = within(lately).getAllByRole('link');
    expect(rows.map((row) => row.getAttribute('href'))).toEqual([
      '/status-pages/page/history/Ended%201',
      '/status-pages/page/history/Ended%202',
      '/status-pages/page/history/Ended%203',
      '/status-pages/page/history/Ended%204',
    ]);
    expect(rows[0]?.textContent).toContain('lasted 2 hr');
  });

  it('says how many components are kept off the public page', () => {
    renderOverview({
      components: [component('API'), component('Internal queue', { visibility: 'hidden' })],
    });

    const unwell = screen.getByRole('heading', { name: 'Not operational' }).closest('section') as HTMLElement;
    expect(within(unwell).getByText('1 hidden component is not on the public page.')).toBeTruthy();
    expect(screen.getByText('1 component on the page')).toBeTruthy();
  });

  it('asks for a first component before it offers to publish anything', () => {
    renderOverview({ components: [] });

    expect(screen.getByRole('button', { name: 'Add component' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'New incident' })).toBeNull();
  });

  it('offers one primary action, and none to a reader who cannot publish', () => {
    renderOverview({}, false);

    const incident = screen.getByRole('button', { name: 'New incident' }) as HTMLButtonElement;
    const maintenance = screen.getByRole('button', { name: 'Schedule maintenance' }) as HTMLButtonElement;
    expect(incident.disabled).toBe(true);
    expect(maintenance.disabled).toBe(true);
    expect(incident.className).toContain('bg-indigo');
    expect(maintenance.className).not.toContain('bg-indigo');
  });
});
