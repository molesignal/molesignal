import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MemoryRouter, Route, Routes } from 'react-router-dom';

import * as statusPagesApi from '@/api/statusPages';
import type { PublicStatusPageSnapshot } from '@/api/statusPages';
import i18n from '@/i18n';

import { PublicStatusIncidentPage } from './IncidentPage';

vi.mock('@/api/statusPages', () => ({
  getPublic: vi.fn(),
  getPublicByDomain: vi.fn(),
  publicRssUrl: vi.fn(() => '/api/v1/public/status-pages/acme-cloud/feed.rss'),
}));

const getPublic = vi.mocked(statusPagesApi.getPublic);

const snapshot: PublicStatusPageSnapshot = {
  page: {
    name: 'Acme Cloud',
    slug: 'acme-cloud',
    logo_url: null,
    brand_color: '#4F46E5',
    timezone: 'UTC',
    language: 'en-us',
    languages: ['en-us', 'zh-cn'],
    history_days: 90,
    visibility: 'public',
  },
  overall_status: 'partial_outage',
  components: [
    {
      id: 'notification-service',
      name: 'Notification Service',
      description: 'Email, webhook, and chat delivery.',
      status: 'partial_outage',
    },
  ],
  component_status_events: [],
  active_incidents: [
    {
      id: 'notification-outage',
      kind: 'incident',
      title: 'Notification Service partial outage',
      impact: 'major',
      status: 'in_progress',
      component_ids: ['notification-service'],
      updates: [
        {
          id: 'update-2',
          status: 'identified',
          message: 'We identified a queue processing issue.',
          created_at: Date.UTC(2026, 7, 10, 7, 20) * 1_000,
        },
        {
          id: 'update-1',
          status: 'investigating',
          message: 'Email notifications may be delayed.',
          created_at: Date.UTC(2026, 7, 10, 7, 6) * 1_000,
        },
        {
          id: 'update-3',
          status: 'in_progress',
          message: 'A fix is being deployed and traffic is recovering.',
          created_at: Date.UTC(2026, 7, 10, 7, 45) * 1_000,
        },
      ],
      started_at: Date.UTC(2026, 7, 10, 7, 6) * 1_000,
      ended_at: null,
    },
  ],
  scheduled_maintenance: [
    {
      id: 'queue-migration',
      kind: 'maintenance',
      title: 'Queue migration',
      impact: 'maintenance',
      status: 'scheduled',
      component_ids: ['notification-service'],
      updates: [],
      started_at: Date.UTC(2026, 7, 12, 7, 50) * 1_000,
      ended_at: null,
    },
  ],
  history: [
    {
      id: 'webhook-failures',
      kind: 'incident',
      title: 'Webhook deliveries failing',
      impact: 'critical',
      status: 'resolved',
      component_ids: ['notification-service'],
      updates: [
        {
          id: 'resolved',
          status: 'resolved',
          message: 'Deliveries are succeeding again.',
          created_at: Date.UTC(2026, 7, 3, 9, 30) * 1_000,
        },
      ],
      started_at: Date.UTC(2026, 7, 3, 8) * 1_000,
      ended_at: Date.UTC(2026, 7, 3, 9, 30) * 1_000,
    },
  ],
  updated_at: Date.UTC(2026, 7, 10, 7, 48) * 1_000,
  generated_at: Date.UTC(2026, 7, 10, 7, 50) * 1_000,
};

function renderIncident(incidentId = 'notification-outage') {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter
        initialEntries={[
          `/status/acme-cloud/incidents/${incidentId}?lang=en-us`,
        ]}
      >
        <Routes>
          <Route
            path="/status/:slug/incidents/:incidentId"
            element={<PublicStatusIncidentPage />}
          />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe('PublicStatusIncidentPage', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
    getPublic.mockReset();
  });

  afterEach(() => cleanup());

  it('says what is happening, with the latest update first', async () => {
    getPublic.mockResolvedValue(snapshot);
    renderIncident();

    expect(
      await screen.findByRole('heading', {
        name: 'Notification Service partial outage',
      }),
    ).toBeTruthy();
    expect(getPublic).toHaveBeenCalledWith('acme-cloud');
    expect(screen.getByText('Major impact')).toBeTruthy();
    expect(screen.getByText('Notification Service')).toBeTruthy();
    // It is still going on: it has a start, and how long ago that was, but no end and no duration.
    expect(screen.getByText('Started').nextElementSibling?.textContent).toContain('44 minutes ago');
    expect(screen.queryByText('Duration')).toBeNull();

    const updates = screen.getByRole('region', { name: 'Updates' });
    const latest = within(updates).getByText('A fix is being deployed and traffic is recovering.');
    const second = within(updates).getByText('We identified a queue processing issue.');
    const first = within(updates).getByText('Email notifications may be delayed.');
    expect(latest.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(second.compareDocumentPosition(first) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // Every update is said once: none of them is lifted out into a field of its own.
    expect(screen.getAllByText('Email notifications may be delayed.')).toHaveLength(1);

    expect(
      screen.getByRole('link', { name: 'Back to status page' }).getAttribute('href'),
    ).toBe('/status/acme-cloud?lang=en-us');
  });

  it('gives something that is over its end and how long it lasted', async () => {
    getPublic.mockResolvedValue(snapshot);
    renderIncident('webhook-failures');

    expect(await screen.findByRole('heading', { name: 'Webhook deliveries failing' })).toBeTruthy();
    expect(screen.getByText('Critical impact')).toBeTruthy();
    const facts = screen.getByText('Started').closest('dl') as HTMLElement;
    expect(within(facts).getByText('Resolved').nextElementSibling?.querySelector('time')?.getAttribute('dateTime')).toBe(
      '2026-08-03T09:30:00.000Z',
    );
    expect(within(facts).getByText('Duration').nextElementSibling?.textContent).toBe('1 hr 30 min');
  });

  it('gives maintenance that is only announced the time it is planned for', async () => {
    getPublic.mockResolvedValue(snapshot);
    renderIncident('queue-migration');

    expect(await screen.findByRole('heading', { name: 'Queue migration' })).toBeTruthy();
    const planned = screen.getByText('Scheduled for').nextElementSibling;
    expect(planned?.querySelector('time')?.getAttribute('dateTime')).toBe('2026-08-12T07:50:00.000Z');
    expect(planned?.textContent).toContain('in 2 days');
    expect(screen.queryByText('Started')).toBeNull();
    expect(screen.getByText('No public updates have been posted yet.')).toBeTruthy();
  });

  it('keeps the public page shell when an incident cannot be found', async () => {
    getPublic.mockResolvedValue(snapshot);
    renderIncident('missing-incident');

    expect(await screen.findByRole('heading', { name: 'Acme Cloud' })).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Incident unavailable' })).toBeTruthy();
    expect(screen.getByText(/does not exist/)).toBeTruthy();
  });
});
