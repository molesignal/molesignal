import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, useLocation } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as statusPagesApi from '@/api/statusPages';
import type { StatusPage, StatusPageSnapshot } from '@/api/statusPages';
import i18n from '@/i18n';
import { useActionAccess } from '@/product/actionAccess';

import { StatusPages } from '.';

vi.mock('@/api/statusPages', () => ({
  list: vi.fn(),
  get: vi.fn(),
  create: vi.fn(),
  archive: vi.fn(),
  restore: vi.fn(),
  remove: vi.fn(),
  uploadLogo: vi.fn(),
}));
vi.mock('@/product/actionAccess', () => ({
  useActionAccess: vi.fn(),
}));

const list = vi.mocked(statusPagesApi.list);
const get = vi.mocked(statusPagesApi.get);
const access = vi.mocked(useActionAccess);
const now = Date.UTC(2026, 7, 10, 12) * 1_000;

function statusPage(id: string, overrides: Partial<StatusPage> = {}): StatusPage {
  return {
    id,
    org_id: 'org',
    name: id,
    slug: id.toLowerCase().replace(/\s+/g, '-'),
    logo_url: null,
    brand_color: '#4F46E5',
    custom_domain: null,
    timezone: 'UTC',
    language: 'en-us',
    languages: ['en-us'],
    history_days: 90,
    delivery_retention_days: 30,
    private_session_days: 7,
    visibility: 'public',
    lifecycle: 'active',
    archived_at: null,
    purge_after: null,
    created_at: now,
    updated_at: now,
    ...overrides,
  };
}

function snapshot(page: StatusPage, overrides: Partial<StatusPageSnapshot> = {}): StatusPageSnapshot {
  const component = (name: string, position: number) => ({
    id: name,
    org_id: 'org',
    status_page_id: page.id,
    name,
    description: '',
    status: 'operational' as const,
    visibility: 'enabled' as const,
    lifecycle: 'active' as const,
    position,
    archived_at: null,
    created_at: now,
    updated_at: now,
  });

  return {
    page,
    overall_status: 'operational',
    components: [component('API', 0), component('Dashboard', 1)],
    component_status_events: [],
    active_incidents: [],
    scheduled_maintenance: [],
    history: [],
    updated_at: now,
    generated_at: now,
    ...overrides,
  };
}

function Where() {
  const location = useLocation();
  return <output data-testid="where">{`${location.pathname}${location.search}`}</output>;
}

function renderList(entry = '/status-pages') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[entry]}>
        <StatusPages />
        <Where />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe('StatusPages', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
    access.mockReturnValue({ allowed: true, disabled: false, reason: undefined } as ReturnType<typeof useActionAccess>);
  });

  afterEach(() => {
    vi.clearAllMocks();
    cleanup();
  });

  it('lists each page as what its readers see on it now', async () => {
    const cloud = statusPage('Acme Cloud');
    const internal = statusPage('Internal', { visibility: 'private' });
    list.mockResolvedValue([cloud, internal]);
    get.mockImplementation(async (pageId) =>
      pageId === cloud.id
        ? snapshot(cloud, {
            overall_status: 'partial_outage',
            active_incidents: [
              {
                id: 'errors',
                org_id: 'org',
                status_page_id: cloud.id,
                source_incident_id: null,
                kind: 'incident',
                title: 'Errors',
                impact: 'major',
                status: 'identified',
                publication_state: 'published',
                draft_message: null,
                component_ids: ['API'],
                updates: [],
                started_at: now,
                ended_at: null,
                published_at: now,
                created_at: now,
                updated_at: now,
              },
            ],
          })
        : snapshot(internal),
    );
    renderList();

    const first = (await screen.findByRole('link', { name: 'Acme Cloud' })).closest('li') as HTMLElement;
    expect(within(first).getByRole('link', { name: 'Acme Cloud' }).getAttribute('href')).toBe(
      '/status-pages/Acme Cloud/overview',
    );
    expect(await within(first).findByText('Partial outage')).toBeTruthy();
    expect(within(first).getByText('1 under way')).toBeTruthy();
    expect(within(first).getByText('2 components')).toBeTruthy();
    // The component the incident is on is drawn in the state the public page shows it in.
    expect(within(first).getByRole('img', { name: /Operational 1, .*Partial outage 1/ })).toBeTruthy();
    // Being public is the rule and is not marked; being private is.
    expect(within(first).queryByText('Private')).toBeNull();

    const second = screen.getByRole('link', { name: 'Internal' }).closest('li') as HTMLElement;
    expect(within(second).getByText('Private')).toBeTruthy();
    expect(await within(second).findByText('Operational')).toBeTruthy();
    expect(within(second).getByText('Nothing under way')).toBeTruthy();

    expect(screen.getAllByRole('button', { name: 'Create status page' })).toHaveLength(1);
  });

  it('says what a status page is before there is one, with the one way forward', async () => {
    const user = userEvent.setup();
    list.mockResolvedValue([]);
    renderList();

    const firstRun = await screen.findByTestId('status-pages-first-run');
    expect(within(firstRun).getByRole('heading', { level: 2 }).textContent).toContain('before they ask');
    // The first run carries the action; the header does not offer it a second time.
    expect(screen.getAllByRole('button', { name: 'Create status page' })).toHaveLength(1);
    expect(screen.queryByRole('group', { name: 'Status page lifecycle' })).toBeNull();

    await user.click(within(firstRun).getByRole('button', { name: 'Archived pages' }));
    expect(screen.getByTestId('where').textContent).toBe('/status-pages?lifecycle=archived');
  });

  it('keeps archived pages apart, with when each is removed for good', async () => {
    list.mockResolvedValue([
      statusPage('Acme Classic', {
        lifecycle: 'archived',
        archived_at: Date.UTC(2026, 7, 1) * 1_000,
        purge_after: Date.UTC(2026, 7, 31) * 1_000,
      }),
    ]);
    renderList('/status-pages?lifecycle=archived');

    const row = (await screen.findByRole('link', { name: 'Acme Classic' })).closest('li') as HTMLElement;
    expect(row.textContent).toMatch(/Archived 2026-08-01/);
    expect(row.textContent).toMatch(/removed for good after 2026-08-31/);
    expect(within(row).getByRole('button', { name: 'Restore' })).toBeTruthy();
    expect(within(row).getByRole('button', { name: 'Delete' })).toBeTruthy();
    // An archived page has no live state to read.
    expect(get).not.toHaveBeenCalled();
    expect(list).toHaveBeenCalledWith('archived');
  });

  it('says so when there are no archived pages, with the view still at hand', async () => {
    list.mockResolvedValue([]);
    renderList('/status-pages?lifecycle=archived');

    expect(await screen.findByText('No archived status pages')).toBeTruthy();
    expect(screen.getByRole('group', { name: 'Status page lifecycle' })).toBeTruthy();
    expect(screen.queryByTestId('status-pages-first-run')).toBeNull();
  });
});
