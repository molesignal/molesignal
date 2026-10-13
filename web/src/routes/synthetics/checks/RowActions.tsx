import { Copy, Ellipsis, Pause, Pencil, Play, Trash2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { restrictActionAccess, type ActionAccess } from '@/product/actionAccess';
import { IconButton } from '@/shell/chrome';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';

import type { CheckRow } from '../data';

/**
 * What can be done to a check from its row: run it now, which is what a person
 * reading the list most often wants, and the rest behind one menu.
 */
export function CheckRowActions({
  row,
  access,
  onRun,
  onEdit,
  onClone,
  onToggle,
  onDelete,
}: {
  row: CheckRow;
  access: ActionAccess;
  onRun: () => void;
  onEdit: () => void;
  onClone: () => void;
  onToggle: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation('synthetics');
  const { monitor } = row;
  const paused = monitor.lifecycle === 'paused';
  const published = Boolean(monitor.active_revision_id);
  // A heartbeat is reported to MoleSignal: there is nothing for it to run.
  const runAccess = restrictActionAccess(
    access,
    published && monitor.lifecycle === 'active' && monitor.kind !== 'heartbeat',
  );

  return (
    <div
      className="flex items-center justify-end gap-[2px]"
      onClick={(event) => event.stopPropagation()}
    >
      <IconButton
        aria-label={t('checks.run_named', { name: monitor.name })}
        title={t('actions.run_now')}
        disabled={runAccess.disabled}
        disabledReason={runAccess.reason}
        onClick={onRun}
      >
        <Play className="h-3.5 w-3.5" />
      </IconButton>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <IconButton
            aria-label={t('checks.more_actions', { name: monitor.name })}
            title={t('checks.columns.actions')}
          >
            <Ellipsis className="h-4 w-4" />
          </IconButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="min-w-40">
          <DropdownMenuItem
            disabled={access.disabled}
            disabledReason={access.reason}
            onSelect={onEdit}
          >
            <Pencil className="h-3.5 w-3.5" />
            {t('actions.edit')}
          </DropdownMenuItem>
          <DropdownMenuItem
            disabled={access.disabled || !row.detail}
            disabledReason={access.reason}
            onSelect={onClone}
          >
            <Copy className="h-3.5 w-3.5" />
            {t('actions.clone')}
          </DropdownMenuItem>
          <DropdownMenuItem
            disabled={access.disabled || !published}
            disabledReason={access.reason}
            onSelect={onToggle}
          >
            {paused ? <Play className="h-3.5 w-3.5" /> : <Pause className="h-3.5 w-3.5" />}
            {t(paused ? 'actions.resume' : 'actions.pause')}
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem
            className="text-red focus:text-red"
            disabled={access.disabled}
            disabledReason={access.reason}
            onSelect={onDelete}
          >
            <Trash2 className="h-3.5 w-3.5" />
            {t('actions.delete')}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
