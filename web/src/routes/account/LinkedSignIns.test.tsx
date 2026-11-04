import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type * as meApi from '@/api/me';
import * as ssoApi from '@/api/sso';
import i18n from '@/i18n';
import { LinkedSignIns } from '@/routes/account/LinkedSignIns';

vi.mock('@/api/sso', () => ({
  listLinkedIdentities: vi.fn(),
  unlinkIdentity: vi.fn(),
}));

vi.mock('@/api/me', async (importOriginal) => ({
  ...(await importOriginal<typeof meApi>()),
  preferences: vi.fn().mockRejectedValue(new Error('offline')),
}));

vi.mock('@/shell/ui/sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

const listLinkedIdentities = vi.mocked(ssoApi.listLinkedIdentities);
const unlinkIdentity = vi.mocked(ssoApi.unlinkIdentity);

function identity(overrides: Partial<ssoApi.LinkedIdentity> = {}): ssoApi.LinkedIdentity {
  return {
    id: 'sso-okta',
    providerId: 'okta',
    providerName: 'Okta',
    kind: 'oidc',
    platform: 'okta',
    orgId: 'acme',
    orgName: 'acme',
    linkedAt: 1_760_000_000_000_000,
    lastLoginAt: 1_761_400_000_000_000,
    canUnlink: true,
    ...overrides,
  };
}

function renderList() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <LinkedSignIns />
    </QueryClientProvider>,
  );
}

describe.sequential('LinkedSignIns', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
    listLinkedIdentities.mockReset();
    unlinkIdentity.mockReset();
  });

  afterEach(() => {
    cleanup();
  });

  it('lists each linked identity with its organization and last use', async () => {
    listLinkedIdentities.mockResolvedValue([identity()]);
    renderList();

    expect(await screen.findByText('Okta')).not.toBeNull();
    expect(screen.getByText(/^acme · Last used /)).not.toBeNull();
    expect(screen.queryByText('None linked')).toBeNull();
  });

  it('says when nothing is linked', async () => {
    listLinkedIdentities.mockResolvedValue([]);
    renderList();

    expect(await screen.findByText('None linked')).not.toBeNull();
  });

  it('keeps the only way a password-less account signs in', async () => {
    listLinkedIdentities.mockResolvedValue([identity({ canUnlink: false })]);
    renderList();

    await screen.findByText('Okta');
    expect((screen.getByRole('button', { name: 'Unlink' }) as HTMLButtonElement).disabled).toBe(
      true,
    );
  });

  it('unlinks an identity once confirmed', async () => {
    const user = userEvent.setup();
    listLinkedIdentities.mockResolvedValueOnce([identity()]).mockResolvedValue([]);
    unlinkIdentity.mockResolvedValue();
    renderList();

    await user.click(await screen.findByRole('button', { name: 'Unlink' }));
    const dialog = screen.getByRole('dialog');
    expect(within(dialog).getByText('Unlink Okta?')).not.toBeNull();
    await user.click(within(dialog).getByRole('button', { name: 'Unlink' }));

    await waitFor(() => expect(unlinkIdentity).toHaveBeenCalledWith('sso-okta'));
    expect(await screen.findByText('None linked')).not.toBeNull();
  });
});
