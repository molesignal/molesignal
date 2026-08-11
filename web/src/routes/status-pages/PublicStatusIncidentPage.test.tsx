import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MemoryRouter, Route, Routes } from 'react-router-dom';

import * as statusPagesApi from '@/api/statusPages';
import type { PublicStatusPageSnapshot } from '@/api/statusPages';
import i18n from '@/i18n';

import { PublicStatusIncidentPage } from './PublicStatusIncidentPage';

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
  scheduled_maintenance: [],
  history: [],
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

  it('shows customer-facing incident metadata and a chronological timeline', async () => {
    getPublic.mockResolvedValue(snapshot);
    renderIncident();

    expect(
      await screen.findByRole('heading', {
        name: 'Notification Service partial outage',
      }),
    ).toBeTruthy();
    expect(getPublic).toHaveBeenCalledWith('acme-cloud');
    expect(screen.getByText('Notification Service')).toBeTruthy();
    expect(screen.getAllByText('Email notifications may be delayed.')).toHaveLength(2);
    expect(screen.getByRole('heading', { name: 'Timeline' })).toBeTruthy();

    const first = screen.getAllByText('Email notifications may be delayed.')[1]!;
    const second = screen.getByText('We identified a queue processing issue.');
    const third = screen.getByText('A fix is being deployed and traffic is recovering.');
    expect(first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(second.compareDocumentPosition(third) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

    expect(
      screen.getByRole('link', { name: 'Back to status page' }).getAttribute('href'),
    ).toBe('/status/acme-cloud?lang=en-us');
  });

  it('keeps the public page shell when an incident cannot be found', async () => {
    getPublic.mockResolvedValue(snapshot);
    renderIncident('missing-incident');

    expect(await screen.findByRole('heading', { name: 'Acme Cloud' })).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Incident unavailable' })).toBeTruthy();
    expect(screen.getByText(/does not exist/)).toBeTruthy();
  });
});
