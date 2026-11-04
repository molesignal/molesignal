import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';

import {
  IntakeUsageSettings,
  MAX_RETENTION_DAYS,
  MAX_SERVICES_PER_ORG,
  MIN_RETENTION_DAYS,
  parseIntakeUsageSettings,
} from './IntakeUsageSettings';
import { SettingsSaveStatusProvider } from './SettingsSaveStatus';

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  update: vi.fn(),
  access: null as { status: string; scope: string; permissions: Set<string> } | null,
}));

vi.mock('@/api/instance', () => ({
  getIntakeUsageSettings: mocks.get,
  updateIntakeUsageSettings: mocks.update,
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
          <IntakeUsageSettings />
        </SettingsSaveStatusProvider>
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

const days = () => screen.getByRole('spinbutton', { name: 'Keep records for' });
const services = () => screen.getByRole('spinbutton', { name: 'Services per organization' });
const saveButton = () => screen.getByRole('button', { name: 'Save changes' });

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  mocks.access = { status: 'ready', scope: 'system', permissions: new Set(['sys.settings.limits.manage']) };
  mocks.get.mockResolvedValue({ retentionDays: 90, maxServicesPerOrg: 1000 });
  mocks.update.mockImplementation(async (settings: unknown) => settings);
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('parseIntakeUsageSettings', () => {
  it('takes whole numbers within bounds', () => {
    expect(parseIntakeUsageSettings({ retentionDays: '90', maxServices: '1000' })).toEqual({
      retentionDays: 90,
      maxServicesPerOrg: 1000,
    });
    expect(
      parseIntakeUsageSettings({
        retentionDays: String(MIN_RETENTION_DAYS),
        maxServices: String(MAX_SERVICES_PER_ORG),
      }),
    ).not.toBeNull();
  });

  it('refuses a retention that could not read back a month of budget', () => {
    expect(
      parseIntakeUsageSettings({ retentionDays: String(MIN_RETENTION_DAYS - 1), maxServices: '1000' }),
    ).toBeNull();
    expect(
      parseIntakeUsageSettings({ retentionDays: String(MAX_RETENTION_DAYS + 1), maxServices: '1000' }),
    ).toBeNull();
  });

  it('refuses a number of services out of bounds', () => {
    expect(parseIntakeUsageSettings({ retentionDays: '90', maxServices: '0' })).toBeNull();
    expect(
      parseIntakeUsageSettings({ retentionDays: '90', maxServices: String(MAX_SERVICES_PER_ORG + 1) }),
    ).toBeNull();
    expect(parseIntakeUsageSettings({ retentionDays: '90', maxServices: '1.5' })).toBeNull();
  });
});

describe('IntakeUsageSettings', () => {
  it('saves what changed', async () => {
    renderPage();

    await waitFor(() => expect((days() as HTMLInputElement).value).toBe('90'));
    expect((saveButton() as HTMLButtonElement).disabled).toBe(true);

    fireEvent.change(days(), { target: { value: '120' } });
    fireEvent.change(services(), { target: { value: '500' } });
    fireEvent.click(saveButton());

    await waitFor(() => expect(mocks.update).toHaveBeenCalledTimes(1));

    expect(mocks.update.mock.calls[0]?.[0]).toEqual({ retentionDays: 120, maxServicesPerOrg: 500 });
  });

  it('holds back what is out of bounds and says why', async () => {
    renderPage();

    await waitFor(() => expect((days() as HTMLInputElement).value).toBe('90'));
    fireEvent.change(days(), { target: { value: '7' } });

    expect((saveButton() as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText(/Keep records for 35 to 3650 days/)).toBeTruthy();
  });

  it('cannot be changed without the permission', async () => {
    mocks.access = { status: 'ready', scope: 'system', permissions: new Set() };
    renderPage();

    await waitFor(() => expect((days() as HTMLInputElement).value).toBe('90'));

    expect((days() as HTMLInputElement).disabled).toBe(true);
  });
});
