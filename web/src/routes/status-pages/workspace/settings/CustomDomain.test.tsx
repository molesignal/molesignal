import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';

import { useStatusPageWorkspace } from '../Layout';
import { CustomDomainSettings } from './CustomDomain';

vi.mock('../Layout', () => ({
  useStatusPageWorkspace: vi.fn(),
}));

const workspace = vi.mocked(useStatusPageWorkspace);

function renderSettings(available: boolean) {
  workspace.mockReturnValue({
    pageId: 'page-one',
    domain: null,
    domainCapability: { available },
    snapshot: { page: { lifecycle: 'active' } },
  } as unknown as ReturnType<typeof useStatusPageWorkspace>);
  const client = new QueryClient({
    defaultOptions: { mutations: { retry: false }, queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <CustomDomainSettings />
    </QueryClientProvider>,
  );
}

describe('CustomDomainSettings', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
  });

  afterEach(() => {
    workspace.mockReset();
    cleanup();
  });

  it('explains the server prerequisite and disables configuration when unavailable', () => {
    renderSettings(false);

    expect(screen.getByText('Custom domains need server configuration')).toBeTruthy();
    expect(screen.getByText(/external_url = "https:\/\/molesignal\.example\.com"/)).toBeTruthy();
    expect(screen.getByText(/account_email = "ops@example\.com"/)).toBeTruthy();
    expect((screen.getByRole('textbox', { name: 'Custom domain' }) as HTMLInputElement).disabled)
      .toBe(true);
    expect((screen.getByRole('button', { name: 'Configure domain' }) as HTMLButtonElement).disabled)
      .toBe(true);
  });

  it('enables the hostname field once the server capability is available', () => {
    renderSettings(true);

    expect(screen.queryByText('Custom domains need server configuration')).toBeNull();
    expect((screen.getByRole('textbox', { name: 'Custom domain' }) as HTMLInputElement).disabled)
      .toBe(false);
  });
});
