import type { TFunction } from 'i18next';

import type { SyncReport } from '@/api/agent/skills';

/** One line on how a repository synchronized: what came in, what went, what could not. */
export function syncSummary(t: TFunction<'agent'>, report: SyncReport): string {
  if (report.status === 'failed') {
    return t('settings.skills.repository.sync_failed', { reason: report.problems[0] ?? '' });
  }
  const line = t('settings.skills.repository.sync_done', {
    imported: report.imported,
    updated: report.updated,
    removed: report.removed,
  });
  return report.problems.length > 0
    ? `${line} ${t('settings.skills.repository.sync_problems', { count: report.problems.length })}`
    : line;
}
