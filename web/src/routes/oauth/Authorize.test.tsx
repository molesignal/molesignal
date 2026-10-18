import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MemoryRouter } from 'react-router-dom';

import * as oauthApi from '@/api/inboundMcpOAuth';
import * as meApi from '@/api/me';
import i18n from '@/i18n';

import { InboundMcpOAuthAuthorize } from './Authorize';

vi.mock('@/api/inboundMcpOAuth', () => ({
  inspectAuthorization: vi.fn(),
  decideAuthorization: vi.fn(),
}));

vi.mock('@/api/me', () => ({
  profile: vi.fn(),
}));

const inspectAuthorization = vi.mocked(oauthApi.inspectAuthorization);
const decideAuthorization = vi.mocked(oauthApi.decideAuthorization);

function inspection(
  overrides: Partial<oauthApi.OAuthAuthorizationInspection> = {},
): oauthApi.OAuthAuthorizationInspection {
  return {
    client: {
      client_id: 'codex-client',
      client_name: 'Codex',
      client_uri: null,
      metadata_document: false,
    },
    scope: 'mcp offline_access',
    resource: 'https://molesignal.example/api/v1/mcp',
    redirect_uri: 'http://127.0.0.1:51173/callback',
    state: null,
    organization: { id: 'org-internal-id', name: 'Acme Production' },
    ...overrides,
  };
}

function renderAuthorization() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const params = new URLSearchParams({
    response_type: 'code',
    client_id: 'codex-client',
    redirect_uri: 'http://127.0.0.1:51173/callback',
    scope: 'mcp offline_access',
    code_challenge: 'A'.repeat(43),
    code_challenge_method: 'S256',
    resource: 'https://molesignal.example/api/v1/mcp',
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[`/oauth/authorize?${params.toString()}`]}>
        <InboundMcpOAuthAuthorize />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe('InboundMcpOAuthAuthorize', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
    inspectAuthorization.mockResolvedValue(inspection());
    vi.mocked(meApi.profile).mockResolvedValue({
      user_id: 'user-1',
      email: 'ada@example.com',
      display_name: 'Ada Lovelace',
      org_id: 'org-internal-id',
      org_name: 'Acme Production',
      org_slug: 'acme',
      display_role: 'Owner',
    });
  });

  afterEach(() => {
    inspectAuthorization.mockReset();
    decideAuthorization.mockReset();
    cleanup();
  });

  it('says who asks for which account and organization, by name', async () => {
    const { container } = renderAuthorization();

    const title = await screen.findByRole('heading', { level: 1 });
    expect(title.textContent).toBe('Codexwants to access your MoleSignal account');
    expect(screen.getByText('Acme Production')).toBeTruthy();
    expect(screen.queryByText('org-internal-id')).toBeNull();
    expect(await screen.findByText('Ada Lovelace')).toBeTruthy();
    expect(screen.getByText('ada@example.com')).toBeTruthy();
    // The client joined to the MoleSignal mark.
    expect(container.querySelector('[data-oauth-marks] svg')).toBeTruthy();
  });

  it('lists what the grant allows, staying signed in only when it was asked for', async () => {
    renderAuthorization();
    expect(await screen.findByText('Use MoleSignal tools as you')).toBeTruthy();
    expect(screen.getByText('Propose changes that run once approved')).toBeTruthy();
    expect(screen.getByText('Keep access until you revoke it')).toBeTruthy();

    cleanup();
    inspectAuthorization.mockResolvedValue(inspection({ scope: 'mcp' }));
    renderAuthorization();
    expect(await screen.findByText('Use MoleSignal tools as you')).toBeTruthy();
    expect(screen.queryByText('Keep access until you revoke it')).toBeNull();
  });

  it('names the origin the browser goes to, and how much the client was checked', async () => {
    renderAuthorization();
    expect(await screen.findByText('Authorizing will redirect to http://127.0.0.1:51173')).toBeTruthy();
    expect(screen.getByText(/Codex registered itself/)).toBeTruthy();

    cleanup();
    inspectAuthorization.mockResolvedValue(
      inspection({
        client: {
          client_id: 'https://cursor.example/oauth/client.json',
          client_name: 'Cursor',
          client_uri: 'https://cursor.example',
          metadata_document: true,
        },
      }),
    );
    renderAuthorization();
    expect(await screen.findByText(/Cursor is described by cursor\.example/)).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Mole Agent → Settings → Inbound MCP' })).toBeTruthy();
  });

  it('sends the decision and follows the redirect MoleSignal returns', async () => {
    const replace = vi.fn();
    vi.stubGlobal('location', { ...window.location, replace });
    decideAuthorization.mockResolvedValue('http://127.0.0.1:51173/callback?code=c&state=s');

    renderAuthorization();
    fireEvent.click(await screen.findByRole('button', { name: 'Authorize' }));

    await waitFor(() => expect(replace).toHaveBeenCalledWith('http://127.0.0.1:51173/callback?code=c&state=s'));
    expect(decideAuthorization.mock.calls[0]?.[1]).toBe(true);
    vi.unstubAllGlobals();
  });
});
