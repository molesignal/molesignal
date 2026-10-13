import type { DisplayReport } from '../reportTypes';

/**
 * What the report workbench is open for: a report that does not exist yet, one the reader
 * can save, or one the reader can only look at.
 */
export type WorkbenchMode = 'new' | 'edit' | 'view';

export function workbenchMode(
  report: DisplayReport | null,
  canSave: boolean,
): WorkbenchMode {
  if (!report) return 'new';
  return canSave ? 'edit' : 'view';
}
