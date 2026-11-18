import type {
  PreapprovalChannel,
  PreapprovalOverview,
  PreapprovalState,
  RemediationAction,
  RemediationPreapprovalInput,
  RemediationRisk,
} from '@/api/remediation';

export const PREAPPROVAL_KEY = ['remediation-preapproval'];

/** How long the window of counted runs is, in seconds. */
export const WINDOW_CHOICES = [900, 3_600, 21_600, 86_400, 604_800] as const;

/** How long a pre-approval lasts, in days; `null` keeps it until it is taken away. */
export const EXPIRY_CHOICES = [1, 7, 30, 90, null] as const;

export const MAX_RUNS = 20;
export const MAX_CHANNELS = 5;

const DAY_MICROS = 86_400_000_000;

/** A pre-approval as the dialog edits it. */
export interface PreapprovalDraft {
  /** As typed, comma separated. */
  services: string;
  maxRuns: number;
  windowSecs: number;
  expiresInDays: number | null;
  channels: PreapprovalChannel[];
}

/** How many people pre-approve an action of this risk: the approvals it needs. */
export function approversNeeded(risk: RemediationRisk): number {
  return risk === 'l3' ? 2 : 1;
}

/** The dialog's start: what is set, else the action's services, one run an hour for 30 days. */
export function preapprovalDraftOf(action: RemediationAction, now: number): PreapprovalDraft {
  const set = action.preapproval;

  if (!set) {
    return {
      services: action.services.join(', '),
      maxRuns: 1,
      windowSecs: 3_600,
      expiresInDays: 30,
      channels: [],
    };
  }

  const left = set.expiresAt === null ? null : Math.max(1, Math.ceil((set.expiresAt - now) / DAY_MICROS));

  return {
    services: set.services.join(', '),
    maxRuns: set.maxRuns,
    windowSecs: set.windowSecs,
    // The nearest choice at or above what is left.
    expiresInDays:
      left === null ? null : (EXPIRY_CHOICES.find((days) => days !== null && days >= left) ?? 90),
    channels: set.channels.map((channel) => ({ ...channel })),
  };
}

/** What the server takes. */
export function preapprovalInputOf(draft: PreapprovalDraft, now: number): RemediationPreapprovalInput {
  return {
    services: draft.services
      .split(',')
      .map((service) => service.trim())
      .filter(Boolean),
    maxRuns: Math.min(MAX_RUNS, Math.max(1, Math.round(draft.maxRuns))),
    windowSecs: draft.windowSecs,
    expiresAt: draft.expiresInDays === null ? null : now + draft.expiresInDays * DAY_MICROS,
    channels: draft.channels
      .map((channel) => ({ ...channel, target: channel.target.trim() }))
      .filter((channel) => channel.target !== ''),
  };
}

/** Where an action's pre-approval stands for the page. */
export interface PreapprovalStanding {
  state: PreapprovalState | 'none';
  used: number;
  max: number;
  /** Whether whoever is signed in can be the person it waits for. */
  canConfirm: boolean;
}

export function preapprovalStanding(
  action: RemediationAction,
  overview: PreapprovalOverview | undefined,
  currentUserId: string,
): PreapprovalStanding {
  const set = action.preapproval;

  if (!set) return { state: 'none', used: 0, max: 0, canConfirm: false };

  const known = overview?.actions[action.id];
  const approvers = new Set(set.approvers.map((approver) => approver.userId));
  const state = known?.state ?? (overview?.enabled === false ? 'off' : 'awaiting_approver');

  return {
    state,
    used: known?.used ?? 0,
    max: set.maxRuns,
    canConfirm:
      approvers.size < approversNeeded(action.risk) && currentUserId !== '' && !approvers.has(currentUserId),
  };
}
