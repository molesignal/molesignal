import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type * as meApi from '@/api/me';
import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';

import { SettingsSaveStatusProvider } from './SettingsSaveStatus';
import { parseStreamRetentionDays, StreamRetentionSettings } from './StreamRetentionSettings';

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  update: vi.fn(),
  preview: vi.fn(),
  getSynthetic: vi.fn(),
  updateSynthetic: vi.fn(),
  access: null as { status: string; scope: string; permissions: Set<string> } | null,
}));

vi.mock('@/api/instance', () => ({
  getStreamRetentionSettings: mocks.get,
  updateStreamRetentionSettings: mocks.update,
  previewStreamRetention: mocks.preview,
  getSyntheticRetentionSettings: mocks.getSynthetic,
  updateSyntheticRetentionSettings: mocks.updateSynthetic,
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
          <StreamRetentionSettings />
        </SettingsSaveStatusProvider>
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

const field = () => screen.getByRole('spinbutton', { name: 'Days to keep' });
const section = (input: HTMLElement) => input.closest('[data-settings-section]') as HTMLElement;
const saveButton = () => within(section(field())).getByRole('button', { name: 'Save changes' });
const runsField = () => screen.getByRole('spinbutton', { name: 'Days to keep runs' });
const dialog = () => screen.findByRole('dialog');
const shortenButton = () => within(screen.getByRole('dialog')).getByRole('button', { name: 'Shorten retention' });

/** What a stream would lose: under the line that asks for the days typed, unless told otherwise. */
function estimate(overrides: Record<string, unknown> = {}) {
  return {
    retentionDays: 7,
    previousRetentionDays: 30,
    estimated: true,
    generatedAt: 1_800_000_000_000_000,
    rows: 1_500,
    storedBytes: 2_048,
    earliestAt: 1_790_000_000_000_000,
    streamCount: 1,
    streams: [
      {
        orgId: 'acme',
        orgName: 'Acme',
        id: 's-1',
        name: 'app-logs',
        streamType: 'logs',
        rows: 1_500,
        storedBytes: 2_048,
        earliestAt: 1_790_000_000_000_000,
      },
    ],
    unavailableStreams: 0,
    ...overrides,
  };
}

async function typeDays(days: string) {
  await waitFor(() => expect(field()).toHaveProperty('value', '30'));
  fireEvent.change(field(), { target: { value: days } });
}

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  mocks.access = {
    status: 'ready',
    scope: 'system',
    permissions: new Set(['sys.settings.retention.manage']),
  };
  mocks.get.mockResolvedValue({ retentionDays: 30 });
  mocks.update.mockImplementation(async (settings: { retention_days: number }) => settings);
  mocks.preview.mockResolvedValue(estimate());
  mocks.getSynthetic.mockResolvedValue({ retentionDays: 30 });
  mocks.updateSynthetic.mockImplementation(async (settings: { retention_days: number }) => settings);
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('stream retention', () => {
  it('takes whole days from 1 to 3650 and nothing else', () => {
    expect(parseStreamRetentionDays('1')).toBe(1);
    expect(parseStreamRetentionDays(' 30 ')).toBe(30);
    expect(parseStreamRetentionDays('3650')).toBe(3650);

    for (const value of ['', '0', '-1', '1.5', '1e3', '3651', 'thirty']) {
      expect(parseStreamRetentionDays(value)).toBeNull();
    }
  });

  it('saves the days typed over the stored ones', async () => {
    renderPage();
    await waitFor(() => expect(field()).toHaveProperty('value', '30'));

    fireEvent.change(field(), { target: { value: '90' } });
    expect(screen.getByText('Unsaved changes')).not.toBeNull();
    fireEvent.click(saveButton());

    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith({ retentionDays: 90 }));
    await waitFor(() =>
      expect(within(section(field())).getByText('Settings are in sync')).not.toBeNull(),
    );

    // A longer retention deletes nothing more: it is not looked up, and nothing is asked.
    expect(mocks.preview).not.toHaveBeenCalled();
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('asks about the days once they have stopped changing', async () => {
    renderPage();
    await typeDays('2');
    fireEvent.change(field(), { target: { value: '25' } });

    await screen.findByText('app-logs');
    expect(mocks.preview).toHaveBeenCalledTimes(1);
    expect(mocks.preview.mock.calls[0]?.[0]).toBe(25);
  });

  it('shows the impact by stream and says it is an estimate, before anything is saved', async () => {
    renderPage();
    await typeDays('7');

    await screen.findByText('app-logs');
    expect(screen.getByText('What this change would delete')).not.toBeNull();
    expect(screen.getByText('Estimated from the file catalog')).not.toBeNull();
    expect(mocks.update).not.toHaveBeenCalled();

    // Back at what is stored, there is nothing to show.
    fireEvent.change(field(), { target: { value: '30' } });
    await waitFor(() => expect(screen.queryByText('app-logs')).toBeNull());
  });

  it('keeps Save off until the estimate is in', async () => {
    let answer = (_preview: unknown) => {};

    mocks.preview.mockImplementation(
      () =>
        new Promise((resolve) => {
          answer = resolve;
        }),
    );

    renderPage();
    await typeDays('7');

    expect(saveButton()).toHaveProperty('disabled', true);
    await waitFor(() => expect(mocks.preview).toHaveBeenCalled());
    expect(saveButton()).toHaveProperty('disabled', true);

    answer(estimate());
    await waitFor(() => expect(saveButton()).toHaveProperty('disabled', false));
  });

  it('asks for a click over a small impact, with its facts', async () => {
    renderPage();
    await typeDays('7');
    await screen.findByText('app-logs');

    fireEvent.click(saveButton());

    const asked = await dialog();

    expect(within(asked).getByText('Shorten the default retention from 30 days to 7 days?')).not.toBeNull();
    expect(within(asked).getByText('1.5K')).not.toBeNull();
    expect(within(asked).getByText('2.0 KiB')).not.toBeNull();
    expect(within(asked).queryByTestId('confirm-type-name')).toBeNull();
    expect(mocks.update).not.toHaveBeenCalled();

    fireEvent.click(shortenButton());

    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith({ retentionDays: 7 }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  });

  it('asks for the days typed back over a large impact', async () => {
    mocks.preview.mockResolvedValue(estimate({ storedBytes: 1024 ** 3 }));

    renderPage();
    await typeDays('7');
    await screen.findByText('app-logs');

    fireEvent.click(saveButton());
    await dialog();

    expect(shortenButton()).toHaveProperty('disabled', true);

    fireEvent.change(screen.getByTestId('confirm-type-name'), { target: { value: '7' } });
    expect(shortenButton()).toHaveProperty('disabled', false);
    fireEvent.click(shortenButton());

    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith({ retentionDays: 7 }));
  });

  it('saves without asking when nothing would be deleted', async () => {
    mocks.preview.mockResolvedValue(
      estimate({ rows: 0, storedBytes: 0, earliestAt: null, streamCount: 0, streams: [] }),
    );

    renderPage();
    await typeDays('7');
    await screen.findByText('Nothing to delete right now: no stream holds files from that period.');

    fireEvent.click(saveButton());

    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith({ retentionDays: 7 }));
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('asks for the days typed back when the estimate failed, and draws no figures', async () => {
    mocks.preview.mockRejectedValue(new Error('catalog unavailable'));

    renderPage();
    await typeDays('7');
    await screen.findByRole('alert');

    fireEvent.click(saveButton());

    const asked = await dialog();

    expect(within(asked).getByTestId('confirm-type-name')).not.toBeNull();
    expect(asked.querySelector('[data-confirm-consequences]')).toBeNull();
    expect(mocks.update).not.toHaveBeenCalled();
  });

  it('leaves the runs of synthetic checks to a plain save', async () => {
    renderPage();
    await waitFor(() => expect(runsField()).toHaveProperty('value', '30'));

    fireEvent.change(runsField(), { target: { value: '7' } });
    fireEvent.click(within(section(runsField())).getByRole('button', { name: 'Save changes' }));

    await waitFor(() => expect(mocks.updateSynthetic).toHaveBeenCalledWith({ retentionDays: 7 }));
    expect(mocks.preview).not.toHaveBeenCalled();
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('keeps Save off for days a stream cannot be kept', async () => {
    renderPage();
    await waitFor(() => expect(field()).toHaveProperty('value', '30'));

    fireEvent.change(field(), { target: { value: '0' } });
    expect(screen.getByText('Enter a whole number from 1 to 3650.')).not.toBeNull();
    expect(saveButton()).toHaveProperty('disabled', true);
    expect(mocks.update).not.toHaveBeenCalled();
  });

  it('leaves the field read-only to someone without the platform permission', async () => {
    mocks.access = { status: 'ready', scope: 'system', permissions: new Set() };
    renderPage();

    await waitFor(() => expect(field()).toHaveProperty('value', '30'));
    expect(field()).toHaveProperty('disabled', true);
  });

  it('keeps synthetic runs for good at 0 and saves them on their own', async () => {
    renderPage();
    await waitFor(() => expect(runsField()).toHaveProperty('value', '30'));

    fireEvent.change(runsField(), { target: { value: '0' } });
    fireEvent.click(within(section(runsField())).getByRole('button', { name: 'Save changes' }));

    await waitFor(() => expect(mocks.updateSynthetic).toHaveBeenCalledWith({ retentionDays: 0 }));
    expect(mocks.update).not.toHaveBeenCalled();
  });
});
