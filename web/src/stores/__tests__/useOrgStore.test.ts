import { QueryClient } from '@tanstack/react-query';
import { renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { useAuthStore } from '@/stores/auth';
import { useInvestigationStack } from '@/stores/useInvestigationStack';

vi.mock('@/api/orgs', () => {
  return {
    listOrgs: vi.fn(),
    selectOrg: vi.fn(),
  };
});

const { listOrgs, selectOrg } = await import('@/api/orgs');
const { useCurrentOrgSelection, useOrgStore } = await import('@/stores/useOrgStore');

describe('useOrgStore', () => {
  const ownerRole = {
    id: 'role-owner',
    key: 'owner',
    name: 'Owner',
    builtin: true,
  };
  const operatorRole = {
    id: 'role-operator',
    key: 'operator',
    name: 'Operator',
    builtin: false,
  };
  const platformOwnerRole = {
    id: 'role-platform-owner',
    key: 'platform_owner',
    name: 'Owner',
    builtin: true,
  };

  beforeEach(() => {
    useOrgStore.getState().reset();
    useAuthStore.setState({
      token: 'tok-a',
      ctx: {
        user_id: 'u1',
        org_id: 'org-a',
        display_role: 'Owner',
        roles: [ownerRole],
      },
    });
    useInvestigationStack.getState().reset();
    vi.mocked(listOrgs).mockReset();
    vi.mocked(selectOrg).mockReset();
  });

  afterEach(() => {
    useOrgStore.getState().reset();
  });

  it('loadOrgs fetches once and seeds currentOrgId from auth ctx', async () => {
    vi.mocked(listOrgs).mockResolvedValue([
      { id: 'org-a', name: 'A', roles: [ownerRole], disabled: false },
      { id: 'org-b', name: 'B', roles: [operatorRole], disabled: false },
    ]);
    await useOrgStore.getState().loadOrgs();
    expect(listOrgs).toHaveBeenCalledTimes(1);
    expect(useOrgStore.getState().orgs.map((o) => o.id)).toEqual(['org-a', 'org-b']);
    expect(useOrgStore.getState().currentOrgId).toBe('org-a');
    expect(useOrgStore.getState().loaded).toBe(true);
  });

  it('loadOrgs no-ops when unauthenticated', async () => {
    useAuthStore.setState({ token: null, ctx: null });
    await useOrgStore.getState().loadOrgs();
    expect(listOrgs).not.toHaveBeenCalled();
    expect(useOrgStore.getState().orgs).toEqual([]);
  });

  it('switchOrg clears the query cache and resets the investigation stack', async () => {
    vi.mocked(listOrgs).mockResolvedValue([
      { id: 'org-a', name: 'A', roles: [ownerRole], disabled: false },
      { id: 'org-b', name: 'B', roles: [operatorRole], disabled: false },
    ]);
    await useOrgStore.getState().loadOrgs();

    vi.mocked(selectOrg).mockResolvedValue({
      token: 'tok-b',
      user_id: 'u1',
      org_id: 'org-b',
      display_role: 'Operator',
      roles: [operatorRole],
      system: false,
    });

    // pre-populate query cache + investigation stack so we can assert they reset
    const queryClient = new QueryClient();
    queryClient.setQueryData(['anything'], { a: 1 });
    useInvestigationStack.getState().push({ kind: 'trace', params: { i: 1 } });

    const clearSpy = vi.spyOn(queryClient, 'clear');
    const resetSpy = vi.spyOn(useInvestigationStack.getState(), 'reset');

    const next = await useOrgStore.getState().switchOrg('org-b', { queryClient });
    expect(next.id).toBe('org-b');
    expect(selectOrg).toHaveBeenCalledWith('org-b');
    expect(clearSpy).toHaveBeenCalledTimes(1);
    expect(resetSpy).toHaveBeenCalledTimes(1);
    expect(useAuthStore.getState().token).toBe('tok-b');
    expect(useAuthStore.getState().ctx?.org_id).toBe('org-b');
    expect(useAuthStore.getState().ctx?.org_name).toBe('B');
    expect(useAuthStore.getState().ctx?.display_role).toBe('Operator');
    expect(useAuthStore.getState().ctx?.roles).toEqual([operatorRole]);
    expect(useOrgStore.getState().currentOrgId).toBe('org-b');
    expect(useOrgStore.getState().orgs.map((org) => org.id)).toEqual(['org-b']);
    expect(useOrgStore.getState().loaded).toBe(false);
  });

  it('keeps the current system org named and selectable alongside tenant orgs', () => {
    useAuthStore.setState({
      token: 'system-token',
      ctx: {
        user_id: 'u1',
        org_id: 'system-org-id',
        org_name: '_sys',
        display_role: 'Owner',
        roles: [platformOwnerRole],
        scope: 'system',
      },
    });
    useOrgStore.setState({
      orgs: [
        {
          id: 'default-org-id',
          name: 'default',
          display_role: 'Owner',
          roles: [ownerRole],
          disabled: false,
        },
      ],
      currentOrgId: 'system-org-id',
      loaded: true,
    });

    const { result } = renderHook(() => useCurrentOrgSelection());

    expect(result.current.orgLabel).toBe('_sys');
    expect(result.current.currentOrgId).toBe('system-org-id');
    expect(result.current.orgOptions).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ id: 'system-org-id', name: '_sys' }),
        expect.objectContaining({ id: 'default-org-id', name: 'default' }),
      ]),
    );
  });

  it('switchOrg propagates errors without touching auth', async () => {
    vi.mocked(listOrgs).mockResolvedValue([
      { id: 'org-a', name: 'A', roles: [ownerRole], disabled: false },
      { id: 'org-b', name: 'B', roles: [operatorRole], disabled: false },
    ]);
    await useOrgStore.getState().loadOrgs();

    vi.mocked(selectOrg).mockRejectedValue(new Error('boom'));
    const queryClient = new QueryClient();
    await expect(
      useOrgStore.getState().switchOrg('org-b', { queryClient }),
    ).rejects.toThrow('boom');
    expect(useAuthStore.getState().token).toBe('tok-a');
    expect(useOrgStore.getState().currentOrgId).toBe('org-a');
  });

  it('rejects switching to a disabled organization before calling the API', async () => {
    useOrgStore.getState().setOrgs([
      { id: 'org-a', name: 'A', roles: [ownerRole], disabled: false },
      { id: 'org-b', name: 'B', roles: [operatorRole], disabled: true },
    ]);

    await expect(
      useOrgStore.getState().switchOrg('org-b', { queryClient: new QueryClient() }),
    ).rejects.toThrow('organization is disabled');
    expect(selectOrg).not.toHaveBeenCalled();
  });
});
