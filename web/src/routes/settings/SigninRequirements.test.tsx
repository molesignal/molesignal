import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type * as ssoApi from '@/api/sso';
import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';
import { useAuthStore } from '@/stores/auth';

import { SigninRequirementRows } from './SigninRequirements';

const mocks = vi.hoisted(() => ({
  read: vi.fn(),
  update: vi.fn(),
}));

vi.mock('@/api/mfa', () => ({
  signinPolicy: mocks.read,
  updateSigninPolicy: mocks.update,
}));
vi.mock('@/product/actionAccess', () => ({
  useActionAccess: () => ({ allowed: true, disabled: false }),
}));
vi.mock('@/shell/ui/sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

/** The policy loads over a query, which takes its time on a busy machine. */
const LOADED = { timeout: 20_000 };

function provider(name: string, test?: ssoApi.SsoTestStanding): ssoApi.SsoProvider {
  return {
    id: name,
    orgId: 'org',
    name,
    kind: 'oidc',
    platform: 'custom',
    enabled: true,
    ...(test ? { test } : {}),
  };
}

function renderRows(ssoProviders: ssoApi.SsoProvider[]) {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <TooltipProvider delayDuration={0}>
        <SigninRequirementRows
          permission="org.signin.manage"
          ssoChoice
          ssoProviders={ssoProviders}
        />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

/** Looked up each time: a switch that waits for the policy is replaced once it can be used. */
const toggleOf = (requirement: 'mfa' | 'sso') =>
  within(screen.getByTestId(`signin-requirement-${requirement}`)).getByRole('switch');

async function turnOnSingleSignOn() {
  await vi.waitFor(() => expect(toggleOf('sso')).toHaveProperty('disabled', false), LOADED);
  fireEvent.click(toggleOf('sso'));
  return screen.findByRole('dialog');
}

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  useAuthStore.setState({
    ctx: {
      userId: 'u1',
      orgId: 'org',
      orgName: 'acme',
      displayRole: 'Owner',
      roles: [],
      scope: 'organization',
    },
  } as never);
  mocks.read.mockResolvedValue({ requireMfa: false, ssoOnly: false });
  mocks.update.mockImplementation(async (policy: unknown) => policy);
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('turning on single sign-on', () => {
  it('asks plainly when a provider has passed its test', async () => {
    renderRows([provider('Okta', { status: 'passed', finishedAt: 1 })]);

    const dialog = await turnOnSingleSignOn();

    expect(within(dialog).getByRole('heading').textContent).toBe('Only allow single sign-on?');
    expect(within(dialog).queryByTestId('confirm-type-name')).toBeNull();

    fireEvent.click(within(dialog).getByRole('button', { name: 'Turn on' }));

    await vi.waitFor(() => expect(mocks.update).toHaveBeenCalledOnce());
    expect(mocks.update.mock.calls[0]?.[0]).toEqual({ requireMfa: false, ssoOnly: true });
  });

  it('warns and takes the workspace name when no provider has passed', async () => {
    renderRows([provider('Okta', { status: 'failed' }), provider('Other')]);

    expect(screen.getByTestId('signin-requirement-sso').textContent).toContain(
      'No provider has passed a test sign-in',
    );

    const dialog = await turnOnSingleSignOn();

    expect(within(dialog).getByRole('heading').textContent).toBe(
      'Only allow single sign-on without a passing test?',
    );
    expect(dialog.textContent).toContain('Okta: Failed');
    expect(dialog.textContent).toContain('Other: Not tested');

    const go = () => within(dialog).getByRole('button', { name: 'Turn on anyway' });
    expect(go()).toHaveProperty('disabled', true);
    fireEvent.click(go());
    expect(mocks.update).not.toHaveBeenCalled();

    fireEvent.change(within(dialog).getByTestId('confirm-type-name'), {
      target: { value: 'acme' },
    });
    expect(go()).toHaveProperty('disabled', false);
    fireEvent.click(go());

    await vi.waitFor(() => expect(mocks.update).toHaveBeenCalledOnce());
  });

  it('counts a result on earlier settings as no result', async () => {
    renderRows([provider('Okta', { status: 'stale', testId: 't-1' })]);

    const dialog = await turnOnSingleSignOn();

    expect(dialog.textContent).toContain('Okta: Settings changed');
    expect(within(dialog).getByTestId('confirm-type-name')).toBeTruthy();
  });

  it('keeps asking for a provider when there is none', async () => {
    renderRows([]);

    const row = await screen.findByTestId('signin-requirement-sso');

    expect(row.textContent).toContain('Enable an OIDC, OAuth 2.0 or SAML provider first.');
    expect(row.textContent).not.toContain('No provider has passed');
    expect(within(row).getByRole('switch')).toHaveProperty('disabled', true);
  });

  it('turning it off again asks nothing, tested or not', async () => {
    mocks.read.mockResolvedValue({ requireMfa: false, ssoOnly: true });
    renderRows([provider('Okta')]);

    await vi.waitFor(() => expect(toggleOf('sso').getAttribute('aria-checked')).toBe('true'), LOADED);
    fireEvent.click(toggleOf('sso'));

    expect(screen.queryByRole('dialog')).toBeNull();
    await vi.waitFor(() =>
      expect(mocks.update).toHaveBeenCalledWith({ requireMfa: false, ssoOnly: false }),
    );
  });
});

describe('requiring multi-factor authentication', () => {
  it('is not held back by the state of the providers', async () => {
    renderRows([provider('Okta')]);

    await vi.waitFor(() => expect(toggleOf('mfa')).toHaveProperty('disabled', false), LOADED);
    fireEvent.click(toggleOf('mfa'));

    const dialog = await screen.findByRole('dialog');

    expect(within(dialog).getByRole('heading').textContent).toBe(
      'Require multi-factor authentication?',
    );
    expect(within(dialog).queryByTestId('confirm-type-name')).toBeNull();
  });
});
