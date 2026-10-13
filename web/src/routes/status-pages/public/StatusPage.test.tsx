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

import { PublicStatusPage } from './StatusPage';

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
        {
          id: 'update-2',
          status: 'in_progress',
          message: 'A fix is being rolled out.',
          created_at: Date.UTC(2026, 7, 10, 7, 20) * 1_000,
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
    expect(screen.getByRole('heading', { name: 'Gateway' })).toBeTruthy();
    expect(
      screen.getByRole('img', { name: /Gateway uptime over the past 90 days/ }),
    ).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Past incidents' })).toBeTruthy();
    expect(getPublicByDomain).toHaveBeenCalledOnce();
    expect(getPublic).not.toHaveBeenCalled();
  });

  it('keeps its three places in the header, on the custom-domain root', async () => {
    getPublicByDomain.mockResolvedValue(snapshot);
    renderDomainPage();

    const places = await screen.findByRole('navigation', { name: 'Status page navigation' });
    const current = within(places).getByRole('link', { name: 'Current status' });
    expect(current.getAttribute('href')).toBe('/?lang=en-us');
    expect(current.getAttribute('aria-current')).toBe('page');
    const history = within(places).getByRole('link', { name: 'Incident history' });
    expect(history.getAttribute('href')).toBe('/history?lang=en-us');
    expect(history.getAttribute('aria-current')).toBeNull();
    expect(within(places).getByRole('link', { name: 'Uptime' }).getAttribute('href')).toBe(
      '/uptime?lang=en-us',
    );
  });

  it('says what is going on under the verdict, with only the latest word on it', async () => {
    getPublicByDomain.mockResolvedValue(snapshot);
    renderDomainPage();

    const verdict = (await screen.findByRole('heading', { name: 'Partial service outage' })).closest(
      'section',
    ) as HTMLElement;
    const incident = within(verdict).getByRole('link', { name: 'Notification delivery delays' });
    expect(incident.getAttribute('href')).toBe('/incidents/notification-outage?lang=en-us');
    expect(within(verdict).getByText('Major impact')).toBeTruthy();
    expect(within(verdict).getByText('Fix in progress')).toBeTruthy();

    // The latest update is on the page; the ones before it are a click away, and counted.
    expect(within(verdict).getByText('A fix is being rolled out.')).toBeTruthy();
    expect(screen.queryByText('Email delivery may be delayed.')).toBeNull();
    expect(
      within(verdict).getByRole('link', { name: 'Read all 2 updates' }).getAttribute('href'),
    ).toBe('/incidents/notification-outage?lang=en-us');
  });

  it('puts the worst of what is going on first, and words maintenance as maintenance', async () => {
    getPublicByDomain.mockResolvedValue({
      ...snapshot,
      active_incidents: [
        {
          id: 'database-upgrade',
          kind: 'maintenance',
          title: 'Database upgrade',
          impact: 'maintenance',
          status: 'in_progress',
          component_ids: ['gateway'],
          updates: [],
          started_at: Date.UTC(2026, 7, 10, 7, 25) * 1_000,
          ended_at: null,
        },
        ...snapshot.active_incidents,
      ],
    });
    renderDomainPage();

    const maintenance = await screen.findByRole('link', { name: 'Database upgrade' });
    const incident = screen.getByRole('link', { name: 'Notification delivery delays' });
    expect(
      incident.compareDocumentPosition(maintenance) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();

    const notice = maintenance.closest('article') as HTMLElement;
    expect(within(notice).getByText('Maintenance under way')).toBeTruthy();
    // Planned work is "in progress"; only an incident has a "fix in progress".
    expect(within(notice).getByText('In progress')).toBeTruthy();
    expect(within(notice).queryByText('Fix in progress')).toBeNull();
    // Nothing has been said about it yet, so there is one link to it and no count of updates.
    expect(within(notice).getByRole('link', { name: 'View details' })).toBeTruthy();
  });

  it('announces maintenance that has not begun after what is going on', async () => {
    getPublicByDomain.mockResolvedValue({
      ...snapshot,
      scheduled_maintenance: [
        {
          id: 'network-maintenance',
          kind: 'maintenance',
          title: 'Network maintenance',
          impact: 'maintenance',
          status: 'scheduled',
          component_ids: ['gateway'],
          updates: [
            {
              id: 'announcement',
              status: 'scheduled',
              message: 'Connections may be reset once.',
              created_at: Date.UTC(2026, 7, 9, 9) * 1_000,
            },
          ],
          started_at: Date.UTC(2026, 7, 12, 7, 30) * 1_000,
          ended_at: null,
        },
      ],
    });
    renderDomainPage();

    const planned = await screen.findByRole('region', { name: 'Scheduled maintenance' });
    const link = within(planned).getByRole('link', { name: /Network maintenance/ });
    expect(link.getAttribute('href')).toBe('/incidents/network-maintenance?lang=en-us');
    expect(link.textContent).toContain('Scheduled for');
    expect(link.textContent).toContain('in 2 days');
    expect(link.textContent).toContain('Connections may be reset once.');

    const verdict = screen.getByRole('heading', { name: 'Partial service outage' }).closest(
      'section',
    ) as HTMLElement;
    expect(within(verdict).queryByText('Network maintenance')).toBeNull();
    expect(
      verdict.compareDocumentPosition(planned) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it('lists what is over under the day it ended on', async () => {
    getPublicByDomain.mockResolvedValue(snapshot);
    renderDomainPage();

    const past = await screen.findByRole('region', { name: 'Past incidents' });
    expect(within(past).getByText('Past 14 days')).toBeTruthy();
    expect(within(past).getByRole('heading', { name: 'August 8, 2026' })).toBeTruthy();
    expect(past.querySelectorAll('[data-status-day]')).toHaveLength(1);

    const row = within(past).getByRole('link', { name: /Database index optimization/ });
    expect(row.getAttribute('href')).toBe('/incidents/database-indexing?lang=en-us');
    expect(row.textContent).toContain('1 hr 6 min');
    // What is still going on is said at the top of the page, not listed with the past.
    expect(within(past).queryByText('Notification delivery delays')).toBeNull();
    expect(
      within(past).getByRole('link', { name: 'Full incident history' }).getAttribute('href'),
    ).toBe('/history?lang=en-us');
  });

  it('looks back no further than the page keeps', async () => {
    getPublicByDomain.mockResolvedValue({
      ...snapshot,
      page: { ...snapshot.page, history_days: 1 },
    });
    renderDomainPage();

    const past = await screen.findByRole('region', { name: 'Past incidents' });
    expect(within(past).getByText('Past 1 day')).toBeTruthy();
    expect(within(past).queryByText('Database index optimization')).toBeNull();
    expect(within(past).getByText('No incidents in the past 1 day.')).toBeTruthy();
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
    // The credit at the foot of the page leads to MoleSignal's website, in a tab of its own.
    const credit = screen.getByRole('link', { name: 'Powered by MoleSignal' });
    expect(credit.getAttribute('href')).toBe('https://molesignal.com');
    expect(credit.getAttribute('target')).toBe('_blank');
    expect(credit.getAttribute('rel')).toBe('noopener noreferrer');
  });

  it('submits a double-opt-in request from the header', async () => {
    const user = userEvent.setup();
    subscribePublic.mockResolvedValue({ accepted: true, confirmation_required: true });
    getPublicByDomain.mockResolvedValue(snapshot);
    renderDomainPage();

    await user.click(await screen.findByRole('button', { name: 'Subscribe' }));
    // The dialog is drawn outside the page, so it carries the publisher's colour itself:
    // without it the button that sends the form has no fill to be seen by.
    expect(screen.getByRole('dialog').style.getPropertyValue('--status-brand')).toBe('#4F46E5');
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
