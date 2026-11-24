import type { TFunction } from 'i18next';
import { BellRing, Flag, type LucideIcon, Rocket, Scaling, Server, Settings2, StickyNote, Tag } from 'lucide-react';

import type { ChangeEntry, ChangeKind } from '@/api/changes';

export const CHANGE_ICON: Record<ChangeKind, LucideIcon> = {
  deployment: Rocket,
  configuration: Settings2,
  feature_flag: Flag,
  scaling: Scaling,
  infrastructure: Server,
  annotation: StickyNote,
  version: Tag,
  rule: BellRing,
};

/** Each kind in its own ink; a deployment that failed or was taken back in the ink of trouble. */
export function changeTone(entry: Pick<ChangeEntry, 'kind' | 'status'>): string {
  if (entry.status === 'failed') return 'text-[var(--red-ink)]';
  if (entry.status === 'rolled_back') return 'text-[var(--orange-ink)]';

  switch (entry.kind) {
    case 'deployment':
      return 'text-[var(--blue-ink)]';
    case 'configuration':
    case 'feature_flag':
      return 'text-[var(--purple-ink)]';
    case 'scaling':
    case 'infrastructure':
      return 'text-[var(--green-ink)]';
    default:
      return 'text-tx-2';
  }
}

/** Where a change can be looked at; `null` when it says too little to open anywhere. */
export function changeHref(entry: ChangeEntry): string | null {
  if (entry.kind === 'rule' && entry.ruleId) return `/alerts/rules/${encodeURIComponent(entry.ruleId)}/edit`;
  if (entry.kind === 'annotation') return entry.dashboardId ? `/dashboards/${encodeURIComponent(entry.dashboardId)}` : null;
  if (!entry.service || !entry.namespace || !entry.environment) return null;

  const params = new URLSearchParams({ namespace: entry.namespace, environment: entry.environment });
  return `/apm/services/${encodeURIComponent(entry.service)}/deployments?${params.toString()}`;
}

/** A change in a line: its kind, what it is of, and how it went when not well. */
export function changeSummary(entry: ChangeEntry, t: TFunction): string {
  const kind = t(`changes.kind.${entry.kind}`);
  const subject =
    entry.kind === 'annotation' || entry.kind === 'rule'
      ? entry.title
      : [entry.service, entry.version].filter(Boolean).join(' ') || entry.description;
  const status = entry.status && entry.status !== 'succeeded' ? ` (${t(`changes.status.${entry.status}`)})` : '';

  return subject ? `${kind} · ${subject}${status}` : `${kind}${status}`;
}

/** Where a moment falls across a frame, as a share kept inside it. */
export function shareOf(at: number, start: number, end: number): number {
  const span = Math.max(end - start, 1);
  return Math.min(100, Math.max(0, ((at - start) / span) * 100));
}
