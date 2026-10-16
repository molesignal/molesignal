import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, within } from '@testing-library/react';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as statusPagesApi from '@/api/statusPages';
import type { StatusPageIncident } from '@/api/statusPages';
import i18n from '@/i18n';

import { StatusPageEvents } from './EventsPage';
import { useStatusPageWorkspace } from './Layout';

vi.mock('@/api/statusPages', () => ({
  listEvents: vi.fn(),
}));
vi.mock('./Layout', () => ({
  useStatusPageWorkspace: vi.fn(),
}));
vi.mock('./event/EventDrawer', () => ({
  StatusPageEventDrawer: () => null,
}));

const listEvents = vi.mocked(statusPagesApi.listEvents);
const workspace = vi.mocked(useStatusPageWorkspace);
const now = Date.UTC(2026, 7, 10, 12) * 1_000;

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
    component_ids: [],
    updates: [],
    started_at: now,
    ended_at: null,
    published_at: now,
    created_at: now,
    updated_at: now,
    ...overrides,
  };
}

function Where() {
  const location = useLocation();
  return <output data-testid="where">{`${location.pathname}${location.search}`}</output>;
}

function renderEvents(entry: string) {
  workspace.mockReturnValue({
    pageId: 'page',
    snapshot: { page: { lifecycle: 'active' }, components: [] },
    manageAccess: { allowed: true, disabled: false, reason: undefined },
  } as unknown as ReturnType<typeof useStatusPageWorkspace>);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[entry]}>
        <Routes>
          <Route path="/status-pages/page/incidents" element={<StatusPageEvents kind="incident" />} />
          <Route path="/status-pages/page/maintenance" element={<StatusPageEvents kind="maintenance" />} />
          <Route path="/status-pages/page/history" element={<p>History</p>} />
        </Routes>
        <Where />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe('StatusPageEvents', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
  });

  afterEach(() => {
    vi.clearAllMocks();
    cleanup();
  });

  it('lists what is still open at the head of one list, with its one action', async () => {
    listEvents.mockResolvedValue({ kind: 'incident', view: 'current', total: 1, items: [event('API errors')] });
    const { container } = renderEvents('/status-pages/page/incidents');

    expect(await screen.findByText('API errors')).toBeTruthy();
    expect(listEvents).toHaveBeenCalledWith('page', 'incident', 'current');

    const head = container.querySelector('[data-status-page-list-head]') as HTMLElement;
    const views = within(head).getByRole('group', { name: 'Event views' });
    // What is over is not listed here a second time: the History tab has it.
    expect(within(views).getAllByRole('button')).toHaveLength(2);
    expect(within(views).getByRole('button', { name: /Current/ }).getAttribute('aria-pressed')).toBe('true');
    expect(within(views).getByRole('button', { name: /Drafts/ })).toBeTruthy();
    expect(within(views).queryByRole('button', { name: /Resolved/ })).toBeNull();
    expect(within(head).getByRole('button', { name: 'New incident' })).toBeTruthy();
    expect(screen.getByRole('columnheader', { name: 'Impact' })).toBeTruthy();
  });

  it('leaves the impact column out for maintenance, which has only one', async () => {
    listEvents.mockResolvedValue({
      kind: 'maintenance',
      view: 'upcoming',
      total: 1,
      items: [event('Database upgrade', { kind: 'maintenance', impact: 'maintenance', status: 'scheduled' })],
    });
    renderEvents('/status-pages/page/maintenance');

    expect(await screen.findByText('Database upgrade')).toBeTruthy();
    expect(screen.queryByRole('columnheader', { name: 'Impact' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Schedule maintenance' })).toBeTruthy();
  });

  it.each([
    ['incident', '/status-pages/page/incidents?view=resolved', '/status-pages/page/history?type=incident'],
    ['maintenance', '/status-pages/page/maintenance?view=completed', '/status-pages/page/history?type=maintenance'],
  ] as const)('leads an old link to ended %s events to the record', (_kind, entry, expected) => {
    renderEvents(entry);

    expect(screen.getByTestId('where').textContent).toBe(expected);
    expect(listEvents).not.toHaveBeenCalled();
  });
});
