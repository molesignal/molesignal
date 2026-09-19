import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MemoryRouter } from 'react-router-dom';

import * as oauthApi from '@/api/inboundMcpOAuth';
import i18n from '@/i18n';

import { InboundMcpOAuthAuthorize } from './Authorize';

vi.mock('@/api/inboundMcpOAuth', () => ({
  inspectAuthorization: vi.fn(),
  decideAuthorization: vi.fn(),
}));

const inspectAuthorization = vi.mocked(oauthApi.inspectAuthorization);

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
    inspectAuthorization.mockResolvedValue({
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
      organization: {
        id: 'org-internal-id',
        name: 'Acme Production',
      },
    });
  });

  afterEach(() => {
    inspectAuthorization.mockReset();
    cleanup();
  });

  it('shows the MoleSignal mark and the trusted organization name', async () => {
    const { container } = renderAuthorization();

    expect(await screen.findByText('Acme Production')).toBeTruthy();
    expect(screen.queryByText('org-internal-id')).toBeNull();
    expect(container.querySelector('svg[width="28"][height="28"]')).toBeTruthy();
  });
});
