import type { QueryClient } from '@tanstack/react-query';
import { create } from 'zustand';

import * as orgsApi from '@/api/orgs';
import type { Org } from '@/api/orgs';
import type { AuthContext } from '@/stores/auth';
import {
  decodeAuthTokenClaims,
  normalizeAuthScope,
  useAuthStore,
} from '@/stores/auth';
import { useInvestigationStack } from '@/stores/useInvestigationStack';

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
    if (!auth.token || !auth.ctx) return;
    if (get().loading) return;
    set({ loading: true });
    try {
      const orgs = await orgsApi.listOrgs();
      set({
        orgs: normalizeOrgs(orgs),
        currentOrgId: auth.ctx.org_id,
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
      currentOrgId: state.currentOrgId ?? useAuthStore.getState().ctx?.org_id ?? null,
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
    const resp = await orgsApi.selectOrg(id);
    const claims = decodeAuthTokenClaims(resp.token);
    const auth = useAuthStore.getState();
    const ctx: AuthContext = {
      user_id: resp.user_id,
      org_id: claims?.org_id ?? resp.org_id,
      org_name: resp.org_name ?? target.name,
      display_role: resp.display_role,
      roles: resp.roles,
      scope: normalizeAuthScope(
        claims?.scope ?? (resp.system ? 'system' : 'organization'),
      ),
    };
    if (auth.ctx?.display_name) ctx.display_name = auth.ctx.display_name;
    if (auth.ctx?.email) ctx.email = auth.ctx.email;
    queryClient.clear();
    auth.setSession(resp.token, ctx);
    // The organization directory itself is scope-sensitive: `_sys` can list
    // platform organizations, while a tenant token may only list the user's
    // memberships. Drop the old directory immediately so platform-visible
    // organization names cannot linger after entering a tenant.
    set({
      orgs: [target],
      currentOrgId: resp.org_id,
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
  const currentOrgId = storedOrgId ?? ctx?.org_id ?? null;
  const currentOrg = orgs.find((o) => o.id === currentOrgId) ?? null;
  const orgLabel = currentOrg?.name ?? ctx?.org_name ?? currentOrgId ?? '—';
  const currentOrgFallback =
    currentOrgId && !currentOrg
      ? {
          id: currentOrgId,
          name: orgLabel,
          ...(ctx?.display_role ? { display_role: ctx.display_role } : {}),
          roles: ctx?.roles ?? [],
          system: ctx?.scope === 'system',
          disabled: false,
        }
      : null;
  const orgOptions = normalizeOrgs([
    ...orgs,
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
