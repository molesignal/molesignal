import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MemoryRouter, useLocation } from 'react-router-dom';

import * as statusPagesApi from '@/api/statusPages';
import type { PublicStatusPageSnapshot } from '@/api/statusPages';
import i18n from '@/i18n';

import { PublicStatusPage } from './PublicStatusPage';

vi.mock('@/api/statusPages', () => ({
  getPublic: vi.fn(),
  getPublicByDomain: vi.fn(),
  subscribePublic: vi.fn(),
  confirmPublicSubscription: vi.fn(),
  unsubscribePublic: vi.fn(),
  publicRssUrl: vi.fn(() => '/api/public/status-pages/acme-cloud/feed.rss'),
}));

const getPublic = vi.mocked(statusPagesApi.getPublic);
const getPublicByDomain = vi.mocked(statusPagesApi.getPublicByDomain);
const subscribePublic = vi.mocked(statusPagesApi.subscribePublic);
const confirmPublicSubscription = vi.mocked(statusPagesApi.confirmPublicSubscription);

const snapshot: PublicStatusPageSnapshot = {
  page: {
    name: 'Acme Cloud',
    slug: 'acme-cloud',
    logo_url: null,
    brand_color: '#4F46E5',
    timezone: 'UTC',
    language: 'en-us',
    languages: ['en-us', 'zh-cn'],
    history_days: 30,
    visibility: 'public',
  },
  overall_status: 'partial_outage',
  components: [
    {
      id: 'gateway',
      name: 'Gateway',
      description: 'Public traffic entry point and edge routing.',
      status: 'operational',
    },
    {
      id: 'notification-service',
      name: 'Notification Service',
      description: 'Email, webhook, and chat delivery.',
      status: 'partial_outage',
    },
  ],
  component_status_events: [
    {
      component_id: 'gateway',
      status: 'operational',
      started_at: Date.UTC(2026, 4, 12, 7, 30) * 1_000,
      ended_at: null,
    },
    {
      component_id: 'notification-service',
      status: 'operational',
      started_at: Date.UTC(2026, 4, 12, 7, 30) * 1_000,
      ended_at: null,
    },
  ],
  active_incidents: [
    {
      id: 'notification-outage',
      kind: 'incident',
      title: 'Notification delivery delays',
      impact: 'major',
      status: 'in_progress',
      component_ids: ['notification-service'],
      updates: [
        {
          id: 'update-1',
          status: 'investigating',
          message: 'Email delivery may be delayed.',
          created_at: Date.UTC(2026, 7, 10, 7, 6) * 1_000,
        },
      ],
      started_at: Date.UTC(2026, 7, 10, 7, 6) * 1_000,
      ended_at: null,
    },
  ],
  scheduled_maintenance: [],
  history: [
    {
      id: 'database-indexing',
      kind: 'maintenance',
      title: 'Database index optimization',
      impact: 'maintenance',
      status: 'resolved',
      component_ids: ['gateway'],
      updates: [],
      started_at: Date.UTC(2026, 7, 8, 6, 0) * 1_000,
      ended_at: Date.UTC(2026, 7, 8, 7, 6) * 1_000,
    },
  ],
  updated_at: Date.UTC(2026, 7, 10, 7, 12) * 1_000,
  generated_at: Date.UTC(2026, 7, 10, 7, 30) * 1_000,
};

function renderDomainPage(unmatchedDomain?: ReactNode, initialEntry = '/') {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[initialEntry]}>
        <PublicStatusPage source="domain" unmatchedDomain={unmatchedDomain} />
        <LocationSearch />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

function LocationSearch() {
  const location = useLocation();
  return (
    <output data-testid="location-search">
      {location.search}
      {location.hash}
    </output>
  );
}

describe('PublicStatusPage domain routing', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
    getPublic.mockReset();
    getPublicByDomain.mockReset();
    subscribePublic.mockReset();
    confirmPublicSubscription.mockReset();
  });

  afterEach(() => cleanup());

  it('renders the same public page for a matching request host', async () => {
    getPublicByDomain.mockResolvedValue(snapshot);
    renderDomainPage();

    expect(await screen.findByRole('heading', { name: 'Acme Cloud' })).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Partial service outage' })).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'System status' })).toBeTruthy();
    const lastUpdated = screen.getByText('Last updated').parentElement?.querySelector('time');
    expect(lastUpdated?.getAttribute('dateTime')).toBe('2026-08-10T07:12:00.000Z');
    expect(screen.getByText('Gateway')).toBeTruthy();
    expect(
      screen.getByRole('img', { name: /Gateway uptime over the past 90 days/ }),
    ).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Recent incidents' })).toBeTruthy();
    expect(screen.getByText('30-day history')).toBeTruthy();
    expect(
      screen.getByRole('link', { name: 'Incident history' }).getAttribute('href'),
    ).toBe('/history?lang=en-us');
    expect(screen.getByRole('link', { name: 'Uptime' }).getAttribute('href')).toBe(
      '/uptime?lang=en-us',
    );
    expect(getPublicByDomain).toHaveBeenCalledOnce();
    expect(getPublic).not.toHaveBeenCalled();
  });

  it('keeps incident updates on the detail page and links to the custom-domain route', async () => {
    getPublicByDomain.mockResolvedValue(snapshot);
    renderDomainPage();

    const incidentLink = await screen.findByRole('link', {
      name: /Notification delivery delays/,
    });
    expect(incidentLink.getAttribute('href')).toBe(
      '/incidents/notification-outage?lang=en-us',
    );
    expect(screen.queryByText('Email delivery may be delayed.')).toBeNull();
    expect(screen.getByText('Database index optimization')).toBeTruthy();
  });

  it('groups recent incidents by page-local date without incident cards', async () => {
    getPublicByDomain.mockResolvedValue(snapshot);
    renderDomainPage();

    const recent = await screen.findByRole('region', { name: 'Recent incidents' });
    expect(within(recent).getByRole('heading', { name: 'August 10, 2026' })).toBeTruthy();
    expect(within(recent).getByRole('heading', { name: 'August 8, 2026' })).toBeTruthy();
    expect(within(recent).getAllByTestId('incident-date-group')).toHaveLength(2);

    const incidentLink = within(recent).getByRole('link', {
      name: /Notification delivery delays/,
    });
    expect(incidentLink.classList.contains('rounded-xl')).toBe(false);
    expect(incidentLink.classList.contains('border')).toBe(false);
  });

  it('hides resolved incidents outside the configured history window', async () => {
    getPublicByDomain.mockResolvedValue({
      ...snapshot,
      page: { ...snapshot.page, history_days: 1 },
    });
    renderDomainPage();

    const recent = await screen.findByRole('region', { name: 'Recent incidents' });
    expect(within(recent).getByText('1-day history')).toBeTruthy();
    expect(within(recent).queryByText('Database index optimization')).toBeNull();
    expect(within(recent).getAllByTestId('incident-date-group')).toHaveLength(1);
  });

  it('shows the date, duration, and related incidents when an uptime bar is hovered', async () => {
    const user = userEvent.setup();
    getPublicByDomain.mockResolvedValue(snapshot);
    renderDomainPage();

    const chart = await screen.findByRole('img', {
      name: /Notification Service uptime over the past 90 days/,
    });
    const incidentBar = chart.querySelector('.bg-orange') as HTMLElement;
    expect(incidentBar).toBeTruthy();
    await user.hover(incidentBar);

    const tooltip = await screen.findByRole('tooltip');
    expect(within(tooltip).getByText('Partial outage')).toBeTruthy();
    expect(within(tooltip).getByText('Notification delivery delays')).toBeTruthy();
    expect(tooltip.textContent).toMatch(/\d+ min/);
  });

  it('hands an unassigned host back to the normal application root', async () => {
    getPublicByDomain.mockResolvedValue(null);
    renderDomainPage(<div>Authenticated application</div>);

    expect(await screen.findByText('Authenticated application')).toBeTruthy();
  });

  it('uses the shared language select and updates the language query', async () => {
    getPublicByDomain.mockResolvedValue(snapshot);
    renderDomainPage();

    const trigger = await screen.findByRole('combobox', { name: 'Status page language' });
    const actions = screen.getByTestId('status-page-actions');
    expect(actions.classList.contains('flex-row')).toBe(true);
    expect(actions.contains(screen.getByRole('button', { name: 'Subscribe' }))).toBe(true);
    expect(actions.contains(trigger)).toBe(true);
    expect(document.querySelector('select')).toBeNull();

    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole('option', { name: '简体中文' }));

    await waitFor(() => {
      expect(screen.getByTestId('location-search').textContent).toBe('?lang=zh-cn');
    });
    expect(screen.getByRole('combobox', { name: '状态页语言' }).textContent).toContain('简体中文');
  });

  it('owns a scroll container so the footer remains reachable', async () => {
    getPublicByDomain.mockResolvedValue(snapshot);
    renderDomainPage();

    await screen.findByRole('heading', { name: 'Acme Cloud' });
    const main = screen.getByRole('main');
    expect(main.classList.contains('h-full')).toBe(true);
    expect(main.classList.contains('overflow-y-auto')).toBe(true);
    expect(screen.getByText('Powered by MoleSignal')).toBeTruthy();
  });

  it('submits a double-opt-in request from the header', async () => {
    const user = userEvent.setup();
    subscribePublic.mockResolvedValue({ accepted: true, confirmation_required: true });
    getPublicByDomain.mockResolvedValue(snapshot);
    renderDomainPage();

    await user.click(await screen.findByRole('button', { name: 'Subscribe' }));
    await user.type(screen.getByPlaceholderText('you@example.com'), 'ops@example.com');
    expect(screen.getByRole('link', { name: 'RSS feed' }).getAttribute('href')).toBe(
      '/api/public/status-pages/acme-cloud/feed.rss',
    );
    await user.click(screen.getByRole('button', { name: 'Send confirmation' }));

    await waitFor(() => {
      expect(subscribePublic).toHaveBeenCalledWith(
        'acme-cloud',
        'email',
        'ops@example.com',
      );
    });
    expect(await screen.findByText('Check your destination')).toBeTruthy();
  });

  it('confirms a subscription token from the emailed page link and removes it from the URL', async () => {
    confirmPublicSubscription.mockResolvedValue({
      accepted: true,
      confirmation_required: false,
    });
    getPublicByDomain.mockResolvedValue(snapshot);
    renderDomainPage(
      undefined,
      '/#subscription_action=confirm&subscription_token=token.12345678901234567890',
    );

    expect(await screen.findByText('Notifications are now active.')).toBeTruthy();
    expect(confirmPublicSubscription).toHaveBeenCalledOnce();
    expect(confirmPublicSubscription).toHaveBeenCalledWith(
      'acme-cloud',
      'token.12345678901234567890',
    );
    await waitFor(() => {
      expect(screen.getByTestId('location-search').textContent).toBe('');
    });
  });
});
