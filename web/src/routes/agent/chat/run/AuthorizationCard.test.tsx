import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as mcpApi from '@/api/agent/mcpServers';
import type { PendingAuthorization } from '@/api/agent/runs';
import i18n from '@/i18n';

import { AuthorizationCard } from './AuthorizationCard';

vi.mock('@/api/agent/mcpServers', () => ({ putMyMcpCredential: vi.fn() }));

const putMyMcpCredential = vi.mocked(mcpApi.putMyMcpCredential);

const pending = (overrides: Partial<PendingAuthorization> = {}): PendingAuthorization => ({
  type: 'authorization',
  callId: 'authorize-1',
  tool: 'linear_search_issues',
  serverId: 'mcp-linear',
  server: 'Linear',
  header: 'authorization',
  bearer: true,
  instructions: 'Create a personal API key under Linear → Settings → API.',
  rejected: false,
  ...overrides,
});

function renderCard(props: Partial<React.ComponentProps<typeof AuthorizationCard>> = {}) {
  const onRespond = vi.fn().mockResolvedValue(undefined);
  render(
    <AuthorizationCard
      pending={pending()}
      canAuthorize
      initiatorName="Ada Lovelace"
      toolLabel="Search issues"
      onRespond={onRespond}
      {...props}
    />,
  );
  return onRespond;
}

describe('AuthorizationCard', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
    putMyMcpCredential.mockResolvedValue({
      serverId: 'mcp-linear',
      userId: 'user-1',
      last4: 'f00d',
      updatedAt: 1,
    } as Awaited<ReturnType<typeof mcpApi.putMyMcpCredential>>);
  });

  afterEach(() => {
    putMyMcpCredential.mockReset();
    cleanup();
  });

  it('says which tool needs whose credential, where to get it and how it is sent', () => {
    renderCard();
    expect(screen.getByText('Authorize to continue')).toBeTruthy();
    expect(
      screen.getByText('Search issues needs your own credential for Linear. Save it and the answer goes on.'),
    ).toBeTruthy();
    expect(screen.getByText('Create a personal API key under Linear → Settings → API.')).toBeTruthy();
    expect(screen.getByText(/Sent as Bearer <credential> in the authorization header/)).toBeTruthy();
  });

  it('saves the credential first, then lets the answer go on', async () => {
    const onRespond = renderCard();
    const save = () => screen.getByRole('button', { name: 'Save and continue' });
    expect(save().hasAttribute('disabled')).toBe(true);

    fireEvent.change(screen.getByLabelText('Your credential for Linear'), {
      target: { value: 'lin_api_f00d' },
    });
    // A disabled button is wrapped for its reason: the enabled one is a new element.
    fireEvent.click(save());

    await waitFor(() =>
      expect(onRespond).toHaveBeenCalledWith({ callId: 'authorize-1', authorized: true }),
    );
    expect(putMyMcpCredential).toHaveBeenCalledWith('mcp-linear', 'lin_api_f00d');
    expect(putMyMcpCredential.mock.invocationCallOrder[0]).toBeLessThan(
      onRespond.mock.invocationCallOrder[0] ?? 0,
    );
  });

  it('skips the server without saving anything', async () => {
    const onRespond = renderCard();
    fireEvent.click(screen.getByRole('button', { name: 'Skip' }));

    await waitFor(() =>
      expect(onRespond).toHaveBeenCalledWith({ callId: 'authorize-1', authorized: false }),
    );
    expect(putMyMcpCredential).not.toHaveBeenCalled();
  });

  it('keeps the card when the credential cannot be saved', async () => {
    putMyMcpCredential.mockRejectedValue(new Error('the credential contains characters a header cannot carry'));
    const onRespond = renderCard();
    fireEvent.change(screen.getByLabelText('Your credential for Linear'), { target: { value: 'bad\nvalue' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save and continue' }));

    await waitFor(() => expect(putMyMcpCredential).toHaveBeenCalled());
    expect(onRespond).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Save and continue' })).toBeTruthy();
  });

  it('asks for a new one when the saved credential was turned down', () => {
    renderCard({ pending: pending({ rejected: true, bearer: false, instructions: '' }) });
    expect(
      screen.getByText('Linear turned down the credential you saved. Enter a new one and Search issues runs again.'),
    ).toBeTruthy();
    expect(screen.getByText('Encrypted and used only for your own calls. Sent in the authorization header.')).toBeTruthy();
  });

  it('shows everyone else whose credential it waits for', () => {
    renderCard({ canAuthorize: false });
    expect(screen.getByText('Waiting for Ada Lovelace to authorize: the credential is theirs')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Save and continue' })).toBeNull();
    expect(screen.queryByLabelText('Your credential for Linear')).toBeNull();
  });
});
