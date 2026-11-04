import { create } from 'zustand';

import type { OrgEntryWaiting, SelectOrgResponse } from '@/api/orgs';

/** The person backed out of what entering an organization asked for. Nothing to report. */
export class OrgEntryDeclined extends Error {
  constructor() {
    super('entering the organization was declined');
    this.name = 'OrgEntryDeclined';
  }
}

export function isOrgEntryDeclined(error: unknown): boolean {
  return error instanceof OrgEntryDeclined;
}

interface Asking {
  orgId: string;
  waiting: OrgEntryWaiting;
  /** The organization entered, or `null` when the person backed out. */
  settle: (entered: SelectOrgResponse | null) => void;
}

interface OrgEntryState {
  asking: Asking | null;
  ask: (orgId: string, waiting: OrgEntryWaiting) => Promise<SelectOrgResponse | null>;
}

/**
 * What entering an organization still asks for, while a dialog asks it. Any page can
 * switch organization (`useOrgStore.switchOrg`), so one dialog answers for all of them.
 */
export const useOrgEntry = create<OrgEntryState>((set) => ({
  asking: null,
  ask: (orgId, waiting) =>
    new Promise((resolve) => {
      set({
        asking: {
          orgId,
          waiting,
          settle: (entered) => {
            set({ asking: null });
            resolve(entered);
          },
        },
      });
    }),
}));
