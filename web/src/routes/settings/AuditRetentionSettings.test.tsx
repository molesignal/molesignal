import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type * as meApi from '@/api/me';
import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';

import { AuditRetentionSettings, parseRetentionDays } from './AuditRetentionSettings';
import { SettingsSaveStatusProvider } from './SettingsSaveStatus';

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  update: vi.fn(),
  preview: vi.fn(),
  access: null as { status: string; scope: string; permissions: Set<string> } | null,
}));

vi.mock('@/api/instance', () => ({
  getAuditRetentionSettings: mocks.get,
  updateAuditRetentionSettings: mocks.update,
  previewAuditRetention: mocks.preview,
}));
vi.mock('@/api/me', async (original) => {
  const actual = await original<typeof meApi>();
  return { ...actual, preferences: async () => actual.DEFAULT_USER_PREFERENCES };
});
vi.mock('@/product/access', () => ({
  useProductAccess: () => mocks.access,
  hasPermission: (permission: string, access: typeof mocks.access) =>
    access?.status === 'ready' && access.permissions.has(permission),
}));
vi.mock('@/shell/ui/sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

function renderPage() {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <TooltipProvider delayDuration={0}>
        <SettingsSaveStatusProvider>
          <AuditRetentionSettings />
        </SettingsSaveStatusProvider>
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

const field = () => screen.getByRole('spinbutton', { name: 'Days to keep' });
const saveButton = () => screen.getByRole('button', { name: 'Save changes' });
const shortenButton = () => within(screen.getByRole('dialog')).getByRole('button', { name: 'Shorten retention' });

const counted = (rows: number) => ({
  retentionDays: 30,
  previousRetentionDays: 90,
  rows,
  earliestAt: rows > 0 ? 1_790_000_000_000_000 : null,
});

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  mocks.access = {
    status: 'ready',
    scope: 'system',
    permissions: new Set(['sys.settings.retention.manage']),
  };
  mocks.get.mockResolvedValue({ retentionDays: 90 });
  mocks.update.mockImplementation(async (settings: { retention_days: number }) => settings);
  mocks.preview.mockResolvedValue(counted(0));
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('audit retention', () => {
  it('takes whole days from 0 to 3650 and nothing else', () => {
    expect(parseRetentionDays('0')).toBe(0);
    expect(parseRetentionDays(' 90 ')).toBe(90);
    expect(parseRetentionDays('3650')).toBe(3650);

    for (const value of ['', '-1', '1.5', '1e3', '3651', 'ninety']) {
      expect(parseRetentionDays(value)).toBeNull();
    }
  });

  it('saves the days typed over the stored ones', async () => {
    renderPage();
    await waitFor(() => expect(field()).toHaveProperty('value', '90'));

    fireEvent.change(field(), { target: { value: '30' } });
    expect(screen.getByText('Unsaved changes')).not.toBeNull();
    await screen.findByText('No records would be deleted right now.');
    fireEvent.click(saveButton());

    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith({ retentionDays: 30 }));
    await waitFor(() => expect(screen.getByText('Settings are in sync')).not.toBeNull());
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('asks nothing of a longer retention, nor of keeping records for good', async () => {
    renderPage();
    await waitFor(() => expect(field()).toHaveProperty('value', '90'));

    fireEvent.change(field(), { target: { value: '365' } });
    fireEvent.change(field(), { target: { value: '0' } });
    fireEvent.click(saveButton());

    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith({ retentionDays: 0 }));
    expect(mocks.preview).not.toHaveBeenCalled();
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('says how many records would go, and a few of them ask for a click', async () => {
    mocks.preview.mockResolvedValue(counted(40));

    renderPage();
    await waitFor(() => expect(field()).toHaveProperty('value', '90'));

    fireEvent.change(field(), { target: { value: '30' } });
    await screen.findByText(/This change would delete 40 records, the oldest from/);

    fireEvent.click(saveButton());

    const asked = await screen.findByRole('dialog');

    expect(within(asked).getByText('Shorten audit retention from 90 days to 30 days?')).not.toBeNull();
    expect(within(asked).getByText('40')).not.toBeNull();
    expect(within(asked).queryByTestId('confirm-type-name')).toBeNull();

    fireEvent.click(shortenButton());

    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith({ retentionDays: 30 }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  });

  it('asks for the days typed back over many records', async () => {
    mocks.preview.mockResolvedValue(counted(2_400));

    renderPage();
    await waitFor(() => expect(field()).toHaveProperty('value', '90'));

    fireEvent.change(field(), { target: { value: '30' } });
    await screen.findByText(/2,400 records/);
    fireEvent.click(saveButton());
    await screen.findByRole('dialog');

    expect(shortenButton()).toHaveProperty('disabled', true);

    fireEvent.change(screen.getByTestId('confirm-type-name'), { target: { value: '30' } });
    fireEvent.click(shortenButton());

    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith({ retentionDays: 30 }));
  });

  it('counts from keeping for good, and says so', async () => {
    mocks.get.mockResolvedValue({ retentionDays: 0 });
    mocks.preview.mockResolvedValue(counted(5));

    renderPage();
    await waitFor(() => expect(field()).toHaveProperty('value', '0'));

    fireEvent.change(field(), { target: { value: '90' } });
    await screen.findByText(/5 records/);
    fireEvent.click(saveButton());

    const asked = await screen.findByRole('dialog');

    expect(within(asked).getByText('Stop keeping audit records forever and keep them for 90 days?')).not.toBeNull();
  });

  it('asks for the days typed back when the records could not be counted', async () => {
    mocks.preview.mockRejectedValue(new Error('unavailable'));

    renderPage();
    await waitFor(() => expect(field()).toHaveProperty('value', '90'));

    fireEvent.change(field(), { target: { value: '30' } });
    await screen.findByRole('alert');
    fireEvent.click(saveButton());

    const asked = await screen.findByRole('dialog');

    expect(within(asked).getByTestId('confirm-type-name')).not.toBeNull();
    expect(asked.querySelector('[data-confirm-consequences]')).toBeNull();
  });

  it('says what is wrong and keeps Save off for days out of range', async () => {
    renderPage();
    await waitFor(() => expect(field()).toHaveProperty('value', '90'));

    fireEvent.change(field(), { target: { value: '4000' } });
    expect(screen.getByText('Enter a whole number from 0 to 3650.')).not.toBeNull();
    expect(saveButton()).toHaveProperty('disabled', true);
    expect(mocks.update).not.toHaveBeenCalled();
  });

  it('leaves the field read-only to someone without the platform permission', async () => {
    mocks.access = { status: 'ready', scope: 'system', permissions: new Set() };
    renderPage();

    await waitFor(() => expect(field()).toHaveProperty('value', '90'));
    expect(field()).toHaveProperty('disabled', true);
  });
});
