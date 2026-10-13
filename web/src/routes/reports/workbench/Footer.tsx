import { Loader2, Trash2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { DialogFooter } from '@/shell/ui/dialog';

import type { DisplayReport, ReportFormat } from '../reportTypes';
import type { WorkbenchMode } from './mode';
import { RunAsNote } from '../runAs/RunAsNote';

/**
 * The actions of the report workbench: delete on one side; on the other, whose access the
 * report runs with, stated beside the control that saves it.
 *
 * A reader who can only look at the report gets a way out and no control that saves.
 */
export function WorkbenchFooter({
  mode,
  report,
  format,
  formId,
  invalid,
  saving,
  deleting,
  onDelete,
  onClose,
}: {
  mode: WorkbenchMode;
  report: DisplayReport | null;
  /** The format chosen in the workbench. */
  format: ReportFormat;
  /** The form the save control submits. */
  formId: string;
  invalid: boolean;
  saving: boolean;
  deleting: boolean;
  onDelete: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation('reports');
  const { t: tc } = useTranslation('common');
  const scheduleAccess = useActionAccess({ permission: 'reports.schedule' });
  const deleteAccess = useActionAccess({ permission: 'reports.delete' });
  const viewing = mode === 'view';

  return (
    <DialogFooter className="flex flex-wrap items-center justify-between gap-x-4 gap-y-3 space-x-0 border-t border-bd-0 bg-[var(--functional-surface)] px-6 py-4">
      <div>
        {report && (!viewing || deleteAccess.allowed) && (
          <ChromeButton
            onClick={onDelete}
            disabled={deleting || deleteAccess.disabled}
            disabledReason={!deleting ? deleteAccess.reason : undefined}
            className="text-red-soft hover:text-red-soft"
          >
            {deleting ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Trash2 className="h-4 w-4" />
            )}
            {t('actions.delete')}
          </ChromeButton>
        )}
      </div>
      <div className="flex min-w-0 flex-1 flex-wrap items-center justify-end gap-x-4 gap-y-3">
        <RunAsNote
          report={report}
          format={format}
          canSave={scheduleAccess.allowed}
        />
        <div className="flex shrink-0 items-center gap-2">
          <ChromeButton onClick={onClose}>
            {viewing ? t('actions.close') : t('actions.cancel')}
          </ChromeButton>
          {!viewing && (
            <ChromeButton
              variant="primary"
              type="submit"
              form={formId}
              disabled={saving || scheduleAccess.disabled || invalid}
              disabledReason={
                !saving
                  ? scheduleAccess.reason ??
                    (invalid ? tc('access.form_invalid') : undefined)
                  : undefined
              }
            >
              {saving && <Loader2 className="h-4 w-4 animate-spin" />}
              {report ? t('actions.save_changes') : t('actions.save_and_enable')}
            </ChromeButton>
          )}
        </div>
      </div>
    </DialogFooter>
  );
}
