import { Pencil, Play, Trash2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { LogInspection } from '@/api/logInspections';
import { IconButton } from '@/shell/chrome';

/**
 * What can be done to an inspection from its row: run it once now, change it,
 * delete it. A reader who may not is shown the controls set back, with the
 * reason under the pointer.
 */
export function InspectionRowActions({
  inspection,
  busy,
  deniedReason,
  onRun,
  onEdit,
  onDelete,
}: {
  inspection: LogInspection;
  /** Something is being done to this inspection: its controls wait for it. */
  busy: boolean;
  /** Why the reader may not manage inspections; undefined when they may. */
  deniedReason: string | undefined;
  onRun: () => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation('logs');
  const { t: tc } = useTranslation('common');
  const { name } = inspection;
  const running = Boolean(inspection.active_run);
  const waiting = deniedReason ?? (busy ? tc('access.operation_pending') : undefined);
  const blocked = Boolean(deniedReason) || busy;

  return (
    <div className="flex items-center justify-end gap-[2px]">
      <IconButton
        aria-label={t('inspections.actions.run_named', { name })}
        title={t('inspections.actions.run')}
        disabled={blocked || running}
        disabledReason={waiting ?? (running ? t('inspections.actions.run_active') : undefined)}
        onClick={onRun}
      >
        <Play className="h-3.5 w-3.5" />
      </IconButton>
      <IconButton
        aria-label={t('inspections.actions.edit_named', { name })}
        title={t('inspections.actions.edit')}
        disabled={blocked}
        disabledReason={waiting}
        onClick={onEdit}
      >
        <Pencil className="h-3.5 w-3.5" />
      </IconButton>
      <IconButton
        aria-label={t('inspections.actions.delete_named', { name })}
        title={t('inspections.actions.delete')}
        disabled={blocked}
        disabledReason={waiting}
        onClick={onDelete}
      >
        <Trash2 className="h-3.5 w-3.5" />
      </IconButton>
    </div>
  );
}
