import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as statusPagesApi from '@/api/statusPages';
import type { StatusPageSlackAppSetting } from '@/api/statusPages';
import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';

import { useStatusPageWorkspace } from '../Layout';
import { StatusPageSlackSettings } from './Slack';

vi.mock('@/api/statusPages', () => ({
  getSlackAppSetting: vi.fn(),
  setSlackApp: vi.fn(),
}));
vi.mock('../Layout', () => ({
  useStatusPageWorkspace: vi.fn(),
}));

const getSetting = vi.mocked(statusPagesApi.getSlackAppSetting);
const setSlackApp = vi.mocked(statusPagesApi.setSlackApp);
const workspace = vi.mocked(useStatusPageWorkspace);

const setting: StatusPageSlackAppSetting = {
  connector_id: null,
  redirect_url: 'https://ms.example.com/api/v1/public/slack/callback',
  connectors: [
    { id: 'status-app', name: 'Status app', enabled: true, adds_channels: true },
    { id: 'alerts-bot', name: 'Alerts bot', enabled: true, adds_channels: false },
  ],
};

function renderSettings() {
  workspace.mockReturnValue({
    pageId: 'page-one',
    snapshot: { page: { lifecycle: 'active', visibility: 'public' } },
  } as unknown as ReturnType<typeof useStatusPageWorkspace>);
  const client = new QueryClient({
    defaultOptions: { mutations: { retry: false }, queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <TooltipProvider>
        <MemoryRouter>
          <StatusPageSlackSettings />
        </MemoryRouter>
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

describe('StatusPageSlackSettings', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
  });

  afterEach(() => {
    vi.clearAllMocks();
    cleanup();
  });

  it('names the connector visitors add the page to Slack with', async () => {
    getSetting.mockResolvedValue(setting);
    setSlackApp.mockResolvedValue({ ...setting, connector_id: 'status-app' });
    renderSettings();

    const select = await screen.findByRole('combobox', { name: 'Slack app' });
    expect(select.textContent).toContain('Off');
    expect(screen.getByText(setting.redirect_url!)).toBeTruthy();
    expect((screen.getByRole('button', { name: 'Save changes' }) as HTMLButtonElement).disabled).toBe(true);

    fireEvent.click(select);
    // A connector without a client ID and secret cannot add channels.
    expect(screen.getByRole('option', { name: 'Alerts bot' }).getAttribute('aria-disabled')).toBe('true');
    fireEvent.click(screen.getByRole('option', { name: 'Status app' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    await waitFor(() => expect(setSlackApp).toHaveBeenCalledWith('page-one', 'status-app'));
  });

  it('points to the connectors when there is none and to the public address it needs', async () => {
    getSetting.mockResolvedValue({ connector_id: null, redirect_url: null, connectors: [] });
    renderSettings();

    expect(await screen.findByRole('link', { name: 'Connectors' })).toHaveProperty(
      'pathname',
      '/settings/notify/connectors',
    );
    expect(screen.getByText('Needs http.external_url to be an HTTPS address.')).toBeTruthy();
  });
});
