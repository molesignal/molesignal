import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type * as orgsApi from '@/api/orgs';
import type { Org } from '@/api/orgs';
import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';
import { useAuthStore } from '@/stores/auth';
import { useOrgStore } from '@/stores/useOrgStore';

import { DangerZone } from './DangerZone';

const session = vi.hoisted(() => ({
  access: null as { status: string; scope: string; permissions: Set<string> } | null,
}));

vi.mock('@/product/access', () => ({
  useProductAccess: () => session.access,
  hasPermission: (permission: string, access: typeof session.access) =>
    access?.status === 'ready' && access.permissions.has(permission),
}));
vi.mock('@/shell/ui/sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

const mocks = vi.hoisted(() => ({ removeOrg: vi.fn(), signOut: vi.fn(async () => undefined) }));

vi.mock('@/api/orgs', async (original) => ({
  ...(await original<typeof orgsApi>()),
  removeOrg: mocks.removeOrg,
}));
vi.mock('@/stores/auth/signOut', () => ({ signOut: mocks.signOut }));

const org = (overrides: Partial<Org>): Org => ({ id: 'acme', name: 'acme', roles: [], disabled: false, ...overrides });
const PLATFORM = org({ id: 'sys', name: '_sys', system: true });
const TENANT = org({ id: 'acme', name: 'acme', system: false });

function renderZone() {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <TooltipProvider delayDuration={0}>
        <MemoryRouter initialEntries={['/settings']}>
          <Routes>
            <Route path="/settings" element={<DangerZone />} />
            <Route path="/settings/organization_management" element={<p data-testid="management" />} />
          </Routes>
        </MemoryRouter>
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

const open = () => screen.getByRole('button', { name: 'Open organization management' });

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  session.access = { status: 'ready', scope: 'organization', permissions: new Set(['org.preferences.manage']) };
  useOrgStore.setState({ orgs: [TENANT], loaded: true, loading: false });
});

afterEach(() => {
  cleanup();
  useOrgStore.getState().reset();
});

describe('organization lifecycle', () => {
  it('deletes this organization from inside it for whoever holds org.delete', async () => {
    session.access = {
      status: 'ready',
      scope: 'organization',
      permissions: new Set(['org.preferences.manage', 'org.delete']),
    };
    mocks.removeOrg.mockResolvedValue(undefined);
    useAuthStore.setState({ ctx: { orgId: 'acme', orgName: 'acme' } as never });
    renderZone();

    fireEvent.click(screen.getByRole('button', { name: 'Delete organization' }));

    const asked = await screen.findByRole('dialog');
    const confirm = () => within(asked).getByRole('button', { name: 'Delete' });

    expect(confirm()).toHaveProperty('disabled', true);
    fireEvent.change(within(asked).getByTestId('confirm-type-name'), { target: { value: 'acme' } });
    fireEvent.click(confirm());

    await waitFor(() => expect(mocks.removeOrg).toHaveBeenCalledWith('acme'));
    await waitFor(() => expect(mocks.signOut).toHaveBeenCalled());
  });

  it('takes whoever administers the platform there, from any organization', async () => {
    const switchOrg = vi.fn(async (_id: string) => PLATFORM);
    useOrgStore.setState({ orgs: [PLATFORM, TENANT], switchOrg });
    renderZone();

    // The permission is theirs in the platform organization: nothing here says they lack it.
    expect(open()).toBeEnabled();
    expect(screen.queryByText(/Requires the/)).toBeNull();
    expect(screen.getByText('Takes you to the platform organization, where organizations are managed.')).not.toBeNull();

    fireEvent.click(open());

    await waitFor(() => expect(screen.getByTestId('management')).not.toBeNull());
    expect(switchOrg).toHaveBeenCalledTimes(1);
    expect(switchOrg.mock.calls[0]?.[0]).toBe('sys');
  });

  it('opens it straight away from the platform organization', () => {
    const switchOrg = vi.fn();
    session.access = { status: 'ready', scope: 'system', permissions: new Set(['sys.organizations.delete']) };
    useOrgStore.setState({ orgs: [PLATFORM, TENANT], switchOrg });
    renderZone();

    expect(screen.queryByText(/Takes you to the platform organization/)).toBeNull();
    fireEvent.click(open());

    expect(screen.getByTestId('management')).not.toBeNull();
    expect(switchOrg).not.toHaveBeenCalled();
  });

  it('tells anyone else who can do it, instead of naming a permission they cannot be given here', () => {
    renderZone();

    expect(screen.queryByRole('button')).toBeNull();
    expect(
      screen.getByText('Deleting an organization takes the Delete organization permission, which you do not have.'),
    ).not.toBeNull();
    expect(screen.queryByText(/sys\.organizations\.manage/)).toBeNull();
  });

  it('does not offer the way in through a platform organization that is switched off', () => {
    useOrgStore.setState({ orgs: [{ ...PLATFORM, disabled: true }, TENANT] });
    renderZone();

    expect(screen.queryByRole('button')).toBeNull();
  });

  it('waits for the list of organizations before saying either', () => {
    useOrgStore.setState({ orgs: [], loaded: false, loadOrgs: vi.fn(async () => undefined) });
    renderZone();

    expect(open()).toBeDisabled();
    expect(
      screen.queryByText('Deleting an organization takes the Delete organization permission, which you do not have.'),
    ).toBeNull();
    expect(useOrgStore.getState().loadOrgs).toHaveBeenCalledTimes(1);
  });

  it('stays where it is when the platform organization cannot be entered', async () => {
    const switchOrg = vi.fn(async () => {
      throw new Error('organization is disabled by a platform administrator');
    });
    useOrgStore.setState({ orgs: [PLATFORM, TENANT], switchOrg });
    renderZone();

    fireEvent.click(open());

    await waitFor(() => expect(open()).toBeEnabled());
    expect(switchOrg).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId('management')).toBeNull();
  });
});
