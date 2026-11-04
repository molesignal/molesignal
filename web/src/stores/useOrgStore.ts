import type { QueryClient } from '@tanstack/react-query';
import { create } from 'zustand';

import * as orgsApi from '@/api/orgs';
import type { Org } from '@/api/orgs';
import { type AuthContextInput, useAuthStore } from '@/stores/auth';
import { useInvestigationStack } from '@/stores/useInvestigationStack';
import { OrgEntryDeclined, useOrgEntry } from '@/stores/useOrgEntry';

interface OrgState {
  orgs: Org[];
  currentOrgId: string | null;
  loading: boolean;
  loaded: boolean;
  loadOrgs: () => Promise<void>;
  upsertOrg: (org: Org) => void;
  removeOrg: (id: string) => void;
  setOrgs: (orgs: Org[]) => void;
  switchOrg: (id: string, opts: { queryClient: QueryClient }) => Promise<Org>;
  reset: () => void;
}

export interface CurrentOrgSelection {
  orgs: Org[];
  currentOrgId: string | null;
  currentOrg: Org | null;
  orgLabel: string;
  orgOptions: Org[];
}

function normalizeOrgs(orgs: Org[]): Org[] {
  const byId = new Map<string, Org>();
  for (const org of orgs) {
    byId.set(org.id, org);
  }
  return [...byId.values()].sort((a, b) => a.name.localeCompare(b.name));
}

export const useOrgStore = create<OrgState>((set, get) => ({
  orgs: [],
  currentOrgId: null,
  loading: false,
  loaded: false,
  loadOrgs: async () => {
    const auth = useAuthStore.getState();
    if (!auth.ctx) return;
    if (get().loading) return;
    set({ loading: true });
    try {
      const orgs = await orgsApi.listOrgs();
      set({
        orgs: normalizeOrgs(orgs),
        currentOrgId: auth.ctx.orgId,
        loaded: true,
      });
    } finally {
      set({ loading: false });
    }
  },
  upsertOrg: (org) =>
    set((state) => {
      const existing = state.orgs.filter((item) => item.id !== org.id);
      return {
        orgs: normalizeOrgs([...existing, org]),
        loaded: true,
      };
    }),
  removeOrg: (id) =>
    set((state) => ({
      orgs: state.orgs.filter((item) => item.id !== id),
      currentOrgId: state.currentOrgId === id ? null : state.currentOrgId,
      loaded: true,
    })),
  setOrgs: (orgs) =>
    set((state) => ({
      orgs: normalizeOrgs(orgs),
      currentOrgId: state.currentOrgId ?? useAuthStore.getState().ctx?.orgId ?? null,
      loaded: true,
    })),
  switchOrg: async (id, { queryClient }) => {
    const target = get().orgs.find((o) => o.id === id);
    if (!target) {
      throw new Error(`unknown org: ${id}`);
    }
    if (target.disabled) {
      throw new Error('organization is disabled by a platform administrator');
    }
    // The server rewrites the session cookies for the organization it enters.
    const answer = await orgsApi.selectOrg(id);
    const resp = 'orgId' in answer ? answer : await useOrgEntry.getState().ask(id, answer);
    if (!resp) throw new OrgEntryDeclined();
    const auth = useAuthStore.getState();
    const ctx: AuthContextInput = {
      userId: resp.userId,
      orgId: resp.orgId,
      orgName: resp.orgName ?? target.name,
      displayRole: resp.displayRole,
      roles: resp.roles,
      scope: resp.system ? 'system' : 'organization',
    };
    if (auth.ctx?.displayName) ctx.displayName = auth.ctx.displayName;
    if (auth.ctx?.email) ctx.email = auth.ctx.email;
    queryClient.clear();
    auth.setSession(ctx);
    // The organization directory itself is scope-sensitive: `_sys` can list
    // platform organizations, while a tenant session may only list the user's
    // memberships. Drop the old directory immediately so platform-visible
    // organization names cannot linger after entering a tenant.
    set({
      orgs: [target],
      currentOrgId: resp.orgId,
      loading: false,
      loaded: false,
    });
    useInvestigationStack.getState().reset();
    return target;
  },
  reset: () =>
    set({ orgs: [], currentOrgId: null, loading: false, loaded: false }),
}));

export function useCurrentOrgSelection(): CurrentOrgSelection {
  const ctx = useAuthStore((s) => s.ctx);
  const orgs = useOrgStore((s) => s.orgs);
  const storedOrgId = useOrgStore((s) => s.currentOrgId);
  const currentOrgId = storedOrgId ?? ctx?.orgId ?? null;
  const currentOrg = orgs.find((o) => o.id === currentOrgId) ?? null;
  const orgLabel = currentOrg?.name ?? ctx?.orgName ?? currentOrgId ?? '—';
  const currentOrgFallback =
    currentOrgId && !currentOrg
      ? {
          id: currentOrgId,
          name: orgLabel,
          ...(ctx?.displayRole ? { displayRole: ctx.displayRole } : {}),
          roles: ctx?.roles ?? [],
          system: ctx?.scope === 'system',
          disabled: false,
        }
      : null;
  // The switcher lists where one can go. An organization that is only administered from the
  // platform organization is in its management list, not here.
  const orgOptions = normalizeOrgs([
    ...orgs.filter((org) => org.accessible !== false || org.id === currentOrgId),
    ...(currentOrgFallback ? [currentOrgFallback] : []),
  ]);

  return {
    orgs,
    currentOrgId,
    currentOrg,
    orgLabel,
    orgOptions,
  };
}
