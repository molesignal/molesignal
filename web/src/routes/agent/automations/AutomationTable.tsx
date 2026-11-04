import type { TFunction } from 'i18next';
import { Copy, History, KeyRound, MoreHorizontal, Pencil, Play, Trash2 } from 'lucide-react';
import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable } from '@/admin';
import type { Automation } from '@/api/agent/automations';
import { formatMicrosActive } from '@/lib/time';
import { IconButton } from '@/shell/chrome';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { Switch } from '@/shell/ui/switch';
import { formatRelativeMicros, formatRelativeSpan } from '@/time/relative';

import { triggerSummary } from './model';
import { RunStatusWord } from './RunStatusWord';

// A column says its name on one line: the widths are set for the cells, not for a wrapped head.
const HEADER = 'whitespace-nowrap';

/** Under a trigger that has no time of its own: where the result goes. */
function outputsLine(t: TFunction, automation: Automation): string {
  if (automation.outputs.writeRca) return t('automations.cells.writes_rca');
  const count = automation.outputs.channels.length;
  return count > 0 ? t('automations.cells.channels', { count }) : t('automations.cells.no_output');
}

export interface AutomationTableProps {
  automations: Automation[];
  /** The moment the list was read at: times are told from it. */
  nowMicros: number;
  /** The automation something is being done to, whose controls wait. */
  busyId: string | null;
  /** Whose an automation is, in a word: the team's name, the organization, yours. */
  scopeLabel: (automation: Automation) => string;
  emptyLabel: React.ReactNode;
  onToggle: (automation: Automation, enabled: boolean) => void;
  onRun: (automation: Automation) => void;
  onRuns: (automation: Automation) => void;
  onEdit: (automation: Automation) => void;
  onClone: (automation: Automation) => void;
  onToken: (automation: Automation) => void;
  onDelete: (automation: Automation) => void;
}

/**
 * The automations the reader can see. A row says what it does and whose it
 * is, what starts it, how its last run went and when the next one is.
 */
export function AutomationTable({
  automations,
  nowMicros,
  busyId,
  scopeLabel,
  emptyLabel,
  onToggle,
  onRun,
  onRuns,
  onEdit,
  onClone,
  onToken,
  onDelete,
}: AutomationTableProps) {
  const { t, i18n } = useTranslation('agent');
  const { t: tc } = useTranslation('common');
  const locale = i18n.resolvedLanguage ?? i18n.language;

  return (
    <DataTable
      rows={automations}
      rowKey={(automation) => automation.id}
      emptyLabel={emptyLabel}
      className="min-w-[960px] rounded-none border-0 bg-transparent"
      columns={[
        {
          key: 'name',
          header: t('automations.columns.name'),
          className: 'w-full max-w-0',
          headerClassName: HEADER,
          cell: (automation) => (
            <div data-agent-automation={automation.id} className="min-w-0 py-1">
              <button
                type="button"
                onClick={() => onRuns(automation)}
                className="block max-w-full truncate rounded-sm text-left font-strong text-tx-0 hover:text-indigo-soft focus-visible:bg-bg-3 focus-visible:outline-none"
                title={automation.name}
              >
                {automation.name}
              </button>
              <div className="mt-0.5 truncate text-xs text-tx-2" title={automation.description || undefined}>
                <span className="text-tx-1">{scopeLabel(automation)}</span>
                {automation.description && ` · ${automation.description}`}
              </div>
            </div>
          ),
        },
        {
          key: 'trigger',
          header: t('automations.columns.trigger'),
          width: 196,
          headerClassName: HEADER,
          cell: (automation) => (
            <div className="min-w-0">
              <div className="truncate text-tx-1">{triggerSummary(t, automation.trigger)}</div>
              <div className="mt-0.5 truncate text-xs text-tx-2">
                {automation.trigger.kind === 'schedule'
                  ? automation.enabled && automation.nextRunAt
                    ? t('automations.cells.next', {
                        when: formatRelativeSpan(automation.nextRunAt, nowMicros, locale),
                      })
                    : t('automations.cells.paused')
                  : automation.trigger.kind === 'api'
                    ? automation.apiTokenPrefix
                      ? t('automations.cells.token', { prefix: automation.apiTokenPrefix })
                      : t('automations.cells.no_token')
                    : outputsLine(t, automation)}
              </div>
            </div>
          ),
        },
        {
          key: 'last',
          header: t('automations.columns.last_run'),
          width: 148,
          headerClassName: HEADER,
          cell: (automation) =>
            automation.lastRun ? (
              <div>
                <RunStatusWord status={automation.lastRun.status} />
                <div className="mt-1 text-xs text-tx-2" title={formatMicrosActive(automation.lastRun.at)}>
                  {formatRelativeMicros(automation.lastRun.at, locale, nowMicros)}
                </div>
              </div>
            ) : (
              <span className="text-tx-3">{t('automations.cells.never')}</span>
            ),
        },
        {
          key: 'enabled',
          header: t('automations.columns.enabled'),
          width: 64,
          headerClassName: HEADER,
          cell: (automation) => {
            const allowed = automation.enabled ? automation.moderatable : automation.editable;
            return (
              <Switch
                checked={automation.enabled}
                disabled={!allowed || busyId === automation.id}
                disabledReason={allowed ? undefined : t('automations.denied')}
                aria-label={t('automations.actions.toggle_named', { name: automation.name })}
                onCheckedChange={(enabled) => onToggle(automation, enabled)}
              />
            );
          },
        },
        {
          key: 'actions',
          header: <span className="sr-only">{t('automations.columns.actions')}</span>,
          width: 136,
          className: 'overflow-visible text-right',
          cell: (automation) => {
            const busy = busyId === automation.id;
            const denied = automation.editable ? undefined : t('automations.denied');
            const waiting = denied ?? (busy ? tc('access.operation_pending') : undefined);
            return (
              <div className="flex items-center justify-end gap-[2px]">
                <IconButton
                  aria-label={t('automations.actions.run_named', { name: automation.name })}
                  title={t('automations.actions.run')}
                  disabled={!automation.editable || busy}
                  disabledReason={waiting}
                  onClick={() => onRun(automation)}
                >
                  <Play className="h-3.5 w-3.5" />
                </IconButton>
                <IconButton
                  aria-label={t('automations.actions.runs_named', { name: automation.name })}
                  title={t('automations.actions.runs')}
                  onClick={() => onRuns(automation)}
                >
                  <History className="h-3.5 w-3.5" />
                </IconButton>
                <IconButton
                  aria-label={t('automations.actions.edit_named', { name: automation.name })}
                  title={t('automations.actions.edit')}
                  disabled={!automation.editable || busy}
                  disabledReason={waiting}
                  onClick={() => onEdit(automation)}
                >
                  <Pencil className="h-3.5 w-3.5" />
                </IconButton>
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <IconButton aria-label={t('automations.actions.more_named', { name: automation.name })}>
                      <MoreHorizontal className="h-3.5 w-3.5" />
                    </IconButton>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" className="w-48">
                    <DropdownMenuItem onSelect={() => onClone(automation)}>
                      <Copy className="h-3.5 w-3.5" />
                      {t('automations.actions.clone')}
                    </DropdownMenuItem>
                    {automation.trigger.kind === 'api' && (
                      <DropdownMenuItem disabled={!automation.editable} onSelect={() => onToken(automation)}>
                        <KeyRound className="h-3.5 w-3.5" />
                        {t('automations.actions.token')}
                      </DropdownMenuItem>
                    )}
                    <DropdownMenuItem
                      disabled={!automation.moderatable}
                      onSelect={() => onDelete(automation)}
                      className="text-[var(--red-ink)]"
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                      {t('automations.actions.delete')}
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>
            );
          },
        },
      ]}
    />
  );
}
