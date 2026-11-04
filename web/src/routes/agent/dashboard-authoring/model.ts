import type { DashboardDraftPreview } from '@/api/agent/dashboardAuthoring';

export type DashboardDraftAction =
  | 'open'
  | 'propose'
  | 'execute'
  | 'wait_for_review'
  | 'blocked';

export function dashboardDraftAction(
  draft: DashboardDraftPreview,
  nowMicros = Date.now() * 1000,
): DashboardDraftAction {
  if (draft.status === 'consumed' && draft.dashboardRoute) return 'open';
  if (draft.status !== 'ready' || effectiveDraftExpiry(draft) <= nowMicros) {
    return 'blocked';
  }
  const operation = draft.operation;
  if (!operation) return draft.canPropose ? 'propose' : 'blocked';
  if (operation.status === 'approved') return 'execute';
  if (operation.status === 'pending') return 'wait_for_review';
  if (operation.status === 'executed' && draft.dashboardRoute) return 'open';
  return 'blocked';
}

export function effectiveDraftExpiry(draft: DashboardDraftPreview): number {
  const operationExpiry = draft.operation?.expiresAt;
  return operationExpiry == null
    ? draft.expiresAt
    : Math.min(draft.expiresAt, operationExpiry);
}

export function dashboardCreationIdempotencyKey(
  draftId: string,
  modelHash: string,
): string {
  return `dashboard-draft-${draftId}-${modelHash.slice(0, 16)}`;
}

export function uniqueDraftWarnings(draft: DashboardDraftPreview) {
  const warnings = [...draft.warnings, ...draft.preflight.warnings];
  return warnings.filter(
    (warning, index) =>
      warnings.findIndex(
        (candidate) =>
          candidate.code === warning.code &&
          candidate.path === warning.path &&
          candidate.message === warning.message,
      ) === index,
  );
}

export function preflightRange(draft: DashboardDraftPreview): {
  from: number;
  to: number;
} | null {
  if (draft.preflight.panels.length === 0) return null;
  return {
    from: Math.min(
      ...draft.preflight.panels.map((panel) => panel.testedFrom),
    ),
    to: Math.max(
      ...draft.preflight.panels.map((panel) => panel.testedTo),
    ),
  };
}

export function remainingDuration(
  expiresAtMicros: number,
  nowMicros = Date.now() * 1000,
): string {
  const seconds = Math.max(0, Math.ceil((expiresAtMicros - nowMicros) / 1_000_000));
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  return minutes > 0 ? `${minutes}m ${remainder}s` : `${remainder}s`;
}
