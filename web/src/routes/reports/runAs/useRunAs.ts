import { useAuthStore } from '@/stores/auth';

import type { DisplayReport, ReportFormat } from '../reportTypes';
import { resolveRunAs, type RunAs } from './model';

/**
 * Whose access `report` runs with, as seen by the signed-in user, once it is saved with
 * `format`. `report` is `null` for a report that has not been saved yet.
 */
export function useRunAs(
  report: DisplayReport | null,
  format: ReportFormat,
): RunAs | null {
  const currentUserId = useAuthStore((state) => state.ctx?.user_id ?? '');

  return resolveRunAs({
    saved: report
      ? { format: report.raw.format, userId: report.raw.run_as_user_id }
      : null,
    format,
    currentUserId,
  });
}
