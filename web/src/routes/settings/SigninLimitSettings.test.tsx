import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';

import { SettingsSaveStatusProvider } from './SettingsSaveStatus';
import { parseSigninLimits, SigninLimitSettings } from './SigninLimitSettings';

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  update: vi.fn(),
  access: null as { status: string; scope: string; permissions: Set<string> } | null,
}));

vi.mock('@/api/instance', () => ({
  getSigninLimits: mocks.get,
  updateSigninLimits: mocks.update,
}));
vi.mock('@/product/access', () => ({
  useProductAccess: () => mocks.access,
  hasPermission: (permission: string, access: typeof mocks.access) =>
    access?.status === 'ready' && access.permissions.has(permission),
}));
vi.mock('@/shell/ui/sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

const LIMITS = {
  accountSourceFailures: 5,
  sourceFailures: 30,
  accountFailures: 20,
  accountIntervalSecs: 60,
  lockMinutes: 15,
};

function serves(source: {
  address: string | null;
  counted: boolean;
  behindUnconfiguredProxy: boolean;
}) {
  mocks.get.mockResolvedValue({ ...LIMITS, source });
}

function renderPage() {
  render(
    <MemoryRouter>
      <QueryClientProvider
        client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
      >
        <TooltipProvider delayDuration={0}>
          <SettingsSaveStatusProvider>
            <SigninLimitSettings />
          </SettingsSaveStatusProvider>
        </TooltipProvider>
      </QueryClientProvider>
    </MemoryRouter>,
  );
}

const lockField = () => screen.getByRole('spinbutton', { name: 'Lock and count for' });
const saveButton = () => screen.getByRole('button', { name: 'Save changes' });

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  mocks.access = {
    status: 'ready',
    scope: 'system',
    permissions: new Set(['sys.settings.signin.manage']),
  };
  serves({ address: '8.8.8.8', counted: true, behindUnconfiguredProxy: false });
  mocks.update.mockImplementation(async (limits: unknown) => ({
    ...(limits as object),
    source: { address: '8.8.8.8', counted: true, behindUnconfiguredProxy: false },
  }));
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('SigninLimitSettings', () => {
  it('reads the numbers it was given, off included', () => {
    expect(
      parseSigninLimits({
        accountSource: '0',
        source: ' 30 ',
        account: '20',
        interval: '60',
        lock: '15',
      }),
    ).toEqual({ ...LIMITS, accountSourceFailures: 0 });
  });

  it.each([
    { field: 'source', value: '-1' },
    { field: 'source', value: '1.5' },
    { field: 'account', value: '' },
    { field: 'accountSource', value: '100001' },
    { field: 'interval', value: '0' },
    { field: 'interval', value: '601' },
    { field: 'lock', value: '0' },
    { field: 'lock', value: '1441' },
  ])('refuses $field = "$value"', ({ field, value }) => {
    const draft = { accountSource: '5', source: '30', account: '20', interval: '60', lock: '15' };

    expect(parseSigninLimits({ ...draft, [field]: value })).toBeNull();
  });

  it('saves what was typed and sends only the limits', async () => {
    renderPage();

    await waitFor(() => expect(lockField()).toHaveValue(15));
    expect(saveButton()).toBeDisabled();

    fireEvent.change(lockField(), { target: { value: '30' } });
    expect(saveButton()).toBeEnabled();

    fireEvent.click(saveButton());

    await waitFor(() => expect(mocks.update).toHaveBeenCalledTimes(1));
    expect(mocks.update.mock.calls[0]?.[0]).toEqual({ ...LIMITS, lockMinutes: 30 });
    await waitFor(() => expect(saveButton()).toBeDisabled());
  });

  it('says a value out of bounds and keeps it from being saved', async () => {
    renderPage();

    await waitFor(() => expect(lockField()).toHaveValue(15));
    fireEvent.change(lockField(), { target: { value: '0' } });

    expect(saveButton()).toBeDisabled();
    expect(screen.getByText(/Enter whole numbers/)).toBeInTheDocument();
  });

  it('confirms an address that counts', async () => {
    renderPage();

    expect(await screen.findByText('8.8.8.8')).toBeInTheDocument();
    expect(screen.getByText('Counted by address')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Open Client IP recognition' })).toBeNull();
  });

  it('warns about a proxy nobody set up, on a public address', async () => {
    serves({ address: '1.1.1.1', counted: true, behindUnconfiguredProxy: true });
    renderPage();

    expect(await screen.findByText('Set up trusted proxies')).toBeInTheDocument();
    expect(screen.getByText(/everyone counts as this one address/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Open Client IP recognition' })).toBeInTheDocument();
  });

  it('warns about a proxy nobody set up, on a private address', async () => {
    serves({ address: '10.0.0.1', counted: false, behindUnconfiguredProxy: true });
    renderPage();

    expect(await screen.findByText('Set up trusted proxies')).toBeInTheDocument();
    expect(screen.getByText(/probably behind a proxy/)).toBeInTheDocument();
  });

  it('says a private address counts for nothing until it is allowed', async () => {
    serves({ address: '10.0.0.1', counted: false, behindUnconfiguredProxy: false });
    renderPage();

    expect(await screen.findByText('Account limit only')).toBeInTheDocument();
    expect(screen.getByText(/This is a private address/)).toBeInTheDocument();
  });

  it('says so when no address could be read', async () => {
    serves({ address: null, counted: false, behindUnconfiguredProxy: false });
    renderPage();

    expect(await screen.findByText('Unknown')).toBeInTheDocument();
    expect(screen.getByText(/No address could be read/)).toBeInTheDocument();
  });
});
