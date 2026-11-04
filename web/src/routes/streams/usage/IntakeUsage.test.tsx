import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { IntakeBudget, SourceUsage, UsageOverview } from '@/api/intakeUsage';
import i18n from '@/i18n';
import type * as productAccess from '@/product/access';
import { TooltipProvider } from '@/shell/ui/tooltip';

import { IntakeUsage } from '.';
import { IntakeUsageEntry } from './Entry';

const mocks = vi.hoisted(() => ({
  overview: vi.fn(),
  detail: vi.fn(),
  setBudget: vi.fn(),
  removeBudget: vi.fn(),
  users: vi.fn(),
  access: null as unknown,
  scope: 'organization',
}));

vi.mock('@/api/intakeUsage', () => ({
  overview: mocks.overview,
  detail: mocks.detail,
  setBudget: mocks.setBudget,
  removeBudget: mocks.removeBudget,
}));
vi.mock('@/api/users', () => ({ list: mocks.users }));
vi.mock('@/shell/ui/sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock('@/product/access', async (importOriginal) => ({
  ...(await importOriginal<typeof productAccess>()),
  useProductAccess: () => mocks.access,
}));

const GIB = 1024 ** 3;

function access(permissions: string[], scope = 'organization') {
  return {
    organizationId: 'org-1',
    role: 'Admin',
    scope,
    permissions: new Set(permissions),
    version: 1,
    routeCatalogVersion: 1,
    routes: [{ id: 'stream.usage', pathPattern: '/streams/usage', allowed: permissions.includes('intake_usage.read') }],
    status: 'ready',
  };
}

function budget(fields: Partial<IntakeBudget> = {}): IntakeBudget {
  return {
    id: 'b-1',
    dimension: 'credential',
    sourceKey: 'api_token:t-1',
    period: 'day',
    limitSize: 10 * 1024,
    warnPercent: 80,
    notifyUserIds: ['u-1'],
    updatedAt: 1,
    periodStart: 0,
    usedBytes: 20 * GIB,
    usedComplete: true,
    usedRatio: 2,
    state: 'exceeded',
    ...fields,
  };
}

function source(fields: Partial<SourceUsage> = {}): SourceUsage {
  return {
    sourceKey: 'api_token:t-1',
    label: { kind: 'api_token', subkind: 'personal', name: 'checkout-prod', detail: 'mspat-abc', missing: false },
    budgetable: true,
    intakeBytes: 20 * GIB,
    events: 1000,
    complete: true,
    share: 0.5,
    budget: null,
    ...fields,
  };
}

function overview(fields: Partial<UsageOverview> = {}): UsageOverview {
  return {
    generatedAt: 2_000,
    windowStart: 1_000,
    windowEnd: 2_000,
    windowSecs: 86_400,
    dimension: 'credential',
    available: true,
    meteringStartedAt: 0,
    totalBytes: 40 * GIB,
    totalEvents: 2000,
    truncated: false,
    sources: [source()],
    ...fields,
  };
}

function renderPage(path = '/streams/usage') {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <TooltipProvider delayDuration={0}>
        <MemoryRouter initialEntries={[path]}>
          <IntakeUsage />
        </MemoryRouter>
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

const rowOf = (name: string) => screen.getByText(name, { exact: true }).closest('tr') as HTMLElement;

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  mocks.access = access(['intake_usage.read', 'intake_budgets.manage']);
  mocks.overview.mockResolvedValue(overview());
  mocks.users.mockResolvedValue([
    { id: 'u-1', email: 'a@example.com', displayName: 'Ada', disabled: false, status: 'active' },
    { id: 'u-2', email: 'b@example.com', displayName: 'Bo', disabled: false, status: 'active' },
    { id: 'u-3', email: 'c@example.com', displayName: 'Cy', disabled: true, status: 'active' },
  ]);
  mocks.setBudget.mockImplementation(async (input: object) => ({ ...budget(), ...input }));
  mocks.removeBudget.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('IntakeUsage', () => {
  it('asks for the dimension and window of the address', async () => {
    renderPage('/streams/usage?dimension=service&window=7d');

    await waitFor(() =>
      expect(mocks.overview).toHaveBeenCalledWith({ dimension: 'service', windowSecs: 7 * 86_400 }),
    );
  });

  it('shows the figures of a source and where its budget stands', async () => {
    mocks.overview.mockResolvedValue(overview({ sources: [source({ budget: budget() })] }));
    renderPage();

    const row = within(await waitFor(() => rowOf('checkout-prod')));

    expect(row.getAllByText('20 GiB').length).toBeGreaterThan(0);
    expect(row.getByText('1,000')).toBeTruthy();
    expect(row.getByText('50%')).toBeTruthy();
    expect(row.getByText('10 GiB / day')).toBeTruthy();
    expect(row.getByText('200%')).toBeTruthy();
    expect(row.getByText('Over budget')).toBeTruthy();
  });

  it('draws the dash for figures that are not known, never zero', async () => {
    mocks.overview.mockResolvedValue(
      overview({
        available: false,
        totalBytes: null,
        totalEvents: null,
        sources: [
          source({
            intakeBytes: null,
            events: null,
            share: null,
            budget: budget({ usedBytes: null, usedRatio: null, state: null, usedComplete: false }),
          }),
        ],
      }),
    );
    renderPage();

    const row = within(await waitFor(() => rowOf('checkout-prod')));

    // Intake, events, share, used, budget used and status.
    expect(row.getAllByText('–')).toHaveLength(6);
    expect(row.queryByText('0 B')).toBeNull();
    expect(screen.getByText('Usage cannot be read right now')).toBeTruthy();
  });

  it('says so when the page itself cannot be read', async () => {
    mocks.overview.mockRejectedValue(new Error('backend is away'));
    renderPage();

    await waitFor(() => expect(mocks.overview).toHaveBeenCalled());
    await waitFor(() => expect(screen.queryByRole('table')).toBeNull());
    expect(screen.queryByText('checkout-prod')).toBeNull();
  });

  it('says since when metering counts when it does not cover the window', async () => {
    mocks.overview.mockResolvedValue(
      overview({
        meteringStartedAt: 1_500,
        sources: [source({ complete: false })],
      }),
    );
    renderPage();

    expect(await screen.findByText(/Metered since/)).toBeTruthy();
    expect(within(rowOf('checkout-prod')).getByText('≥ 20 GiB')).toBeTruthy();
  });

  it('sets a budget with who is told', async () => {
    renderPage();

    fireEvent.click(await screen.findByRole('button', { name: 'Set budget' }));

    const drawer = within(await screen.findByRole('dialog'));

    await drawer.findByText('Ada');
    expect(drawer.queryByText('Cy')).toBeNull();

    const save = drawer.getByRole('button', { name: 'Save budget' }) as HTMLButtonElement;
    expect(save.disabled).toBe(true);

    fireEvent.change(drawer.getByTestId('budget-limit'), { target: { value: '5' } });
    fireEvent.click(drawer.getByRole('checkbox', { name: /Bo/ }));
    fireEvent.click(drawer.getByRole('button', { name: 'Save budget' }));

    await waitFor(() => expect(mocks.setBudget).toHaveBeenCalledTimes(1));

    expect(mocks.setBudget.mock.calls[0]?.[0]).toMatchObject({
      dimension: 'credential',
      sourceKey: 'api_token:t-1',
      period: 'day',
      limitSize: 5120,
      warnPercent: 80,
      notifyUserIds: ['u-2'],
    });
  });

  it('fills the form with the budget a source has', async () => {
    mocks.overview.mockResolvedValue(
      overview({ sources: [source({ budget: budget({ period: 'month', limitSize: 2048, warnPercent: 50 }) })] }),
    );
    renderPage();

    fireEvent.click(await screen.findByRole('button', { name: 'Change' }));

    const drawer = within(await screen.findByRole('dialog'));

    expect((drawer.getByTestId('budget-limit') as HTMLInputElement).value).toBe('2');
    expect((drawer.getByTestId('budget-warn') as HTMLInputElement).value).toBe('50');
  });

  it('leaves who is told to the backend when members cannot be read', async () => {
    mocks.users.mockRejectedValue(new Error('forbidden'));
    renderPage();

    fireEvent.click(await screen.findByRole('button', { name: 'Set budget' }));

    const drawer = within(await screen.findByRole('dialog'));

    await drawer.findByText('Members cannot be listed, so you are the one told.');
    fireEvent.change(drawer.getByTestId('budget-limit'), { target: { value: '5' } });
    fireEvent.click(drawer.getByRole('button', { name: 'Save budget' }));

    await waitFor(() => expect(mocks.setBudget).toHaveBeenCalledTimes(1));

    expect(mocks.setBudget.mock.calls[0]?.[0]).not.toHaveProperty('notify_user_ids');
  });

  it('refuses a limit that is not one', async () => {
    renderPage();

    fireEvent.click(await screen.findByRole('button', { name: 'Set budget' }));

    const drawer = within(await screen.findByRole('dialog'));
    const save = drawer.getByRole('button', { name: 'Save budget' }) as HTMLButtonElement;

    fireEvent.change(drawer.getByTestId('budget-limit'), { target: { value: '0' } });
    expect(save.disabled).toBe(true);

    fireEvent.change(drawer.getByTestId('budget-limit'), { target: { value: '1' } });
    fireEvent.change(drawer.getByTestId('budget-warn'), { target: { value: '100' } });
    expect(save.disabled).toBe(true);
  });

  it('offers no budget on what cannot take one', async () => {
    mocks.overview.mockResolvedValue(
      overview({
        sources: [
          source({
            sourceKey: 'internal:',
            label: { kind: 'internal', subkind: null, name: '', detail: null, missing: false },
            budgetable: false,
          }),
        ],
      }),
    );
    renderPage();

    const row = within(await waitFor(() => rowOf('Platform internal')));

    expect(row.queryByRole('button', { name: 'Set budget' })).toBeNull();
    expect(row.queryByRole('checkbox')).toBeNull();
  });

  it('says nothing is recorded yet, rather than a total of zero', async () => {
    mocks.overview.mockResolvedValue(
      overview({
        meteringStartedAt: null,
        totalBytes: null,
        totalEvents: null,
        sources: [source({ intakeBytes: null, events: null, share: null, complete: false })],
      }),
    );
    renderPage();

    expect(await screen.findByText('Nothing has been recorded yet')).toBeTruthy();
    expect(screen.queryByText(/0 B/)).toBeNull();
  });

  it('does not ask for somebody who left, who the backend would refuse', async () => {
    mocks.overview.mockResolvedValue(
      overview({ sources: [source({ budget: budget({ notifyUserIds: ['u-1', 'gone'] }) })] }),
    );
    renderPage();

    fireEvent.click(await screen.findByRole('button', { name: 'Change' }));

    const drawer = within(await screen.findByRole('dialog'));

    await drawer.findByText('Ada');
    fireEvent.click(drawer.getByRole('button', { name: 'Save budget' }));

    await waitFor(() => expect(mocks.setBudget).toHaveBeenCalledTimes(1));

    expect(mocks.setBudget.mock.calls[0]?.[0]).toMatchObject({ notifyUserIds: ['u-1'] });
  });

  it('wants somebody to tell', async () => {
    mocks.overview.mockResolvedValue(
      overview({ sources: [source({ budget: budget({ notifyUserIds: ['gone'] }) })] }),
    );
    renderPage();

    fireEvent.click(await screen.findByRole('button', { name: 'Change' }));

    const drawer = within(await screen.findByRole('dialog'));

    expect(await drawer.findByText('Choose at least one person.')).toBeTruthy();
    expect((drawer.getByRole('button', { name: 'Save budget' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('lets the budget of a source that is gone be removed, not changed', async () => {
    mocks.overview.mockResolvedValue(
      overview({
        sources: [
          source({
            label: { kind: 'api_token', subkind: null, name: 't-1', detail: null, missing: true },
            budgetable: false,
            budget: budget(),
          }),
        ],
      }),
    );
    renderPage();

    const row = within(await waitFor(() => rowOf('t-1')));

    expect(row.queryByRole('button', { name: 'Change' })).toBeNull();

    fireEvent.click(row.getByRole('button', { name: 'Remove budget' }));
    fireEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Remove budget' }));

    await waitFor(() => expect(mocks.removeBudget).toHaveBeenCalledWith('b-1'));
  });

  it('says a credential that is gone is gone', async () => {
    mocks.overview.mockResolvedValue(
      overview({
        sources: [
          source({
            label: { kind: 'api_token', subkind: null, name: 't-1', detail: null, missing: true },
            budgetable: false,
          }),
        ],
      }),
    );
    renderPage();

    expect(within(await waitFor(() => rowOf('t-1'))).getByText('Deleted')).toBeTruthy();
  });

  it('shows the figures but no way to change budgets to who may only read', async () => {
    mocks.access = access(['intake_usage.read']);
    mocks.overview.mockResolvedValue(overview({ sources: [source({ budget: budget() })] }));
    renderPage();

    const row = within(await waitFor(() => rowOf('checkout-prod')));

    expect(row.getByText('10 GiB / day')).toBeTruthy();
    expect(row.queryByRole('button', { name: 'Change' })).toBeNull();
    expect(row.queryByRole('checkbox')).toBeNull();
  });

  it('leaves budgets out in the platform organization', async () => {
    mocks.access = access(['intake_usage.read', 'intake_budgets.manage'], 'system');
    renderPage();

    await waitFor(() => rowOf('checkout-prod'));

    expect(screen.queryByText('Budget')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Set budget' })).toBeNull();
    expect(screen.queryByRole('checkbox')).toBeNull();
  });

  it('opens what a source is made of', async () => {
    mocks.detail.mockResolvedValue({
      generatedAt: 2,
      windowStart: 0,
      windowEnd: 2,
      windowSecs: 86_400,
      meteringStartedAt: 0,
      intakeBytes: 20 * GIB,
      events: 1000,
      buckets: [{ startTime: 0, endTime: 1, intakeBytes: 20 * GIB, events: 1000 }],
      breakdowns: [
        {
          dimension: 'stream',
          sources: [
            {
              sourceKey: 'logs:app-logs',
              label: { kind: 'stream', subkind: 'logs', name: 'app-logs', detail: null, missing: false },
              intakeBytes: 20 * GIB,
              events: 1000,
            },
          ],
        },
        { dimension: 'service', sources: [] },
      ],
    });
    renderPage();

    fireEvent.click(await screen.findByText('checkout-prod'));

    const drawer = within(await screen.findByRole('dialog'));

    expect(await drawer.findByText('app-logs')).toBeTruthy();
    expect(drawer.getByTestId('usage-trend')).toBeTruthy();
    expect(drawer.getByText('Nothing in this window.')).toBeTruthy();
    expect(mocks.detail).toHaveBeenCalledWith({
      dimension: 'credential',
      sourceKey: 'api_token:t-1',
      windowSecs: 86_400,
      bucketCount: 24,
    });
  });
});

describe('IntakeUsageEntry', () => {
  function renderEntry() {
    render(
      <TooltipProvider delayDuration={0}>
        <MemoryRouter>
          <IntakeUsageEntry />
        </MemoryRouter>
      </TooltipProvider>,
    );
  }

  it('is there for who may read usage', () => {
    renderEntry();

    expect(screen.getByRole('button', { name: 'Intake usage' })).toBeTruthy();
  });

  it('is not there for anyone else', () => {
    mocks.access = access(['streams.read']);
    renderEntry();

    expect(screen.queryByRole('button', { name: 'Intake usage' })).toBeNull();
  });
});
