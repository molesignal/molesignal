import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';

import { parseSeriesLimits, SeriesLimitSettings } from './SeriesLimitSettings';
import { SettingsSaveStatusProvider } from './SettingsSaveStatus';

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  update: vi.fn(),
  access: null as { status: string; scope: string; permissions: Set<string> } | null,
}));

vi.mock('@/api/instance', () => ({
  getSeriesLimitSettings: mocks.get,
  updateSeriesLimitSettings: mocks.update,
}));
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
          <SeriesLimitSettings />
        </SettingsSaveStatusProvider>
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

const perOrg = () => screen.getByRole('spinbutton', { name: 'Per organization' });
const perMetric = () => screen.getByRole('spinbutton', { name: 'Per metric' });
const saveButton = () => screen.getByRole('button', { name: 'Save changes' });

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  mocks.access = {
    status: 'ready',
    scope: 'system',
    permissions: new Set(['sys.settings.limits.manage']),
  };
  mocks.get.mockResolvedValue({
    maxActiveSeriesPerOrg: 200000,
    maxActiveSeriesPerMetric: 100000,
  });
  mocks.update.mockImplementation(async (settings: unknown) => settings);
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('series limits', () => {
  it('takes whole counts with a metric allowed at most what its organization is', () => {
    expect(parseSeriesLimits({ perOrg: '10', perMetric: ' 10 ' })).toEqual({
      maxActiveSeriesPerOrg: 10,
      maxActiveSeriesPerMetric: 10,
    });

    const rejected: [string, string][] = [
      ['10', '11'],
      ['0', '0'],
      ['1.5', '1'],
      ['100000001', '1'],
      ['', '1'],
    ];

    for (const [org, metric] of rejected) {
      expect(parseSeriesLimits({ perOrg: org, perMetric: metric })).toBeNull();
    }
  });

  it('saves the limits typed over the stored ones', async () => {
    renderPage();
    await waitFor(() => expect(perOrg()).toHaveProperty('value', '200000'));

    fireEvent.change(perOrg(), { target: { value: '500000' } });
    fireEvent.change(perMetric(), { target: { value: '50000' } });
    fireEvent.click(saveButton());

    await waitFor(() =>
      expect(mocks.update).toHaveBeenCalledWith({
        maxActiveSeriesPerOrg: 500000,
        maxActiveSeriesPerMetric: 50000,
      }),
    );
  });

  it('keeps Save off while a metric would be allowed more than its organization', async () => {
    renderPage();
    await waitFor(() => expect(perMetric()).toHaveProperty('value', '100000'));

    fireEvent.change(perMetric(), { target: { value: '300000' } });
    expect(saveButton()).toHaveProperty('disabled', true);
    expect(mocks.update).not.toHaveBeenCalled();
  });
});
