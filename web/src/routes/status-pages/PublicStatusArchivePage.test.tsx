import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MemoryRouter } from 'react-router-dom';

import * as statusPagesApi from '@/api/statusPages';
import type {
  IncidentImpact,
  PublicStatusPageIncident,
  PublicStatusPageSnapshot,
} from '@/api/statusPages';
import i18n from '@/i18n';

import {
  PublicStatusHistoryPage,
  PublicStatusUptimePage,
} from './PublicStatusArchivePage';

vi.mock('@/api/statusPages', () => ({
  getPublic: vi.fn(),
  getPublicByDomain: vi.fn(),
  publicRssUrl: vi.fn(() => '/api/v1/public/status-pages/acme-cloud/feed.rss'),
}));

const getPublicByDomain = vi.mocked(statusPagesApi.getPublicByDomain);

function incident(
  id: string,
  impact: IncidentImpact,
  resolvedAt: number,
): PublicStatusPageIncident {
  return {
    id,
    kind: impact === 'maintenance' ? 'maintenance' : 'incident',
    title: id,
    impact,
    status: 'resolved',
    component_ids: ['api'],
    updates: [],
    started_at: resolvedAt - 60 * 60 * 1_000_000,
    ended_at: resolvedAt,
  };
}

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
  overall_status: 'operational',
  components: [
    {
      id: 'api',
      name: 'API',
      description: 'Customer API',
      status: 'operational',
    },
  ],
  component_status_events: [],
  active_incidents: [],
  scheduled_maintenance: [],
  history: [
    incident('Critical API outage', 'critical', Date.UTC(2026, 7, 5, 12) * 1_000),
    incident('Major API outage', 'major', Date.UTC(2026, 7, 4, 21) * 1_000),
    incident('Minor API delay', 'minor', Date.UTC(2026, 7, 3, 12) * 1_000),
    incident('API maintenance', 'maintenance', Date.UTC(2026, 7, 2, 12) * 1_000),
    incident('July API delay', 'minor', Date.UTC(2026, 6, 31, 12) * 1_000),
  ],
  updated_at: Date.UTC(2026, 7, 10, 11, 45) * 1_000,
  generated_at: Date.UTC(2026, 7, 10, 12) * 1_000,
};

function renderArchive(view: 'history' | 'uptime') {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const Component = view === 'history' ? PublicStatusHistoryPage : PublicStatusUptimePage;
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[`/${view}?lang=en-us`]}>
        <Component source="domain" />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe('public status archive pages', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
    getPublicByDomain.mockReset();
    getPublicByDomain.mockResolvedValue(snapshot);
  });

  afterEach(() => cleanup());

  it('groups incidents by natural month and previews the three most severe', async () => {
    const user = userEvent.setup();
    renderArchive('history');

    expect(
      await screen.findByRole('heading', { name: 'Incident history' }),
    ).toBeTruthy();
    expect(screen.getByText('June 2026 to August 2026')).toBeTruthy();
    expect(
      (screen.getByRole('button', { name: 'Next month' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    const august = screen.getByRole('region', { name: 'August 2026' });
    expect(within(august).getByText('Critical API outage')).toBeTruthy();
    expect(within(august).getByText('Major API outage')).toBeTruthy();
    expect(within(august).getByText('Minor API delay')).toBeTruthy();
    expect(within(august).queryByText('API maintenance')).toBeNull();

    await user.click(within(august).getByRole('button', { name: /Show all 4/ }));
    expect(within(august).getByText('API maintenance')).toBeTruthy();
    expect(screen.getByRole('region', { name: 'July 2026' })).toBeTruthy();

    await user.click(screen.getByRole('button', { name: 'Previous month' }));
    expect(screen.getByText('May 2026 to July 2026')).toBeTruthy();
    expect(screen.queryByRole('region', { name: 'August 2026' })).toBeNull();
    expect(screen.getByRole('region', { name: 'May 2026' })).toBeTruthy();
    expect(
      (screen.getByRole('button', { name: 'Previous month' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    expect(
      (screen.getByRole('button', { name: 'Next month' }) as HTMLButtonElement)
        .disabled,
    ).toBe(false);
  });

  it('renders timezone-aligned monthly uptime cells with hover details', async () => {
    const user = userEvent.setup();
    renderArchive('uptime');

    expect(
      await screen.findByRole('heading', { name: 'Uptime history' }),
    ).toBeTruthy();
    expect(screen.getByText('June 2026 to August 2026')).toBeTruthy();
    expect(screen.getByRole('combobox', { name: 'Component' }).textContent).toContain(
      'API',
    );
    expect(screen.getAllByRole('gridcell')).toHaveLength(71);

    const affectedDay = screen.getByRole('gridcell', {
      name: 'August 4, 2026: Partial outage',
    });
    await user.hover(affectedDay);
    expect(
      within(await screen.findByRole('tooltip')).getByText('Major API outage'),
    ).toBeTruthy();
  });

  it('keeps all archive links on the custom-domain root', async () => {
    renderArchive('history');

    await screen.findByRole('heading', { name: 'Incident history' });
    expect(screen.getByRole('link', { name: 'Current status' }).getAttribute('href')).toBe(
      '/?lang=en-us',
    );
    expect(screen.getAllByRole('link', { name: 'Incident history' })[0]?.getAttribute('href')).toBe(
      '/history?lang=en-us',
    );
    expect(screen.getAllByRole('link', { name: 'Uptime' })[0]?.getAttribute('href')).toBe(
      '/uptime?lang=en-us',
    );
  });
});
