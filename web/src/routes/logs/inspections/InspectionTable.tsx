import type * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { DataTable } from '@/admin';
import type { LogInspection } from '@/api/logInspections';
import { formatMicrosActive } from '@/lib/time';
import { Switch } from '@/shell/ui/switch';
import { formatRelativeMicros, formatRelativeSpan } from '@/time/relative';

import { StandingMark } from './LevelMark';
import { durationLabel, scheduleLabel, standingOf } from './model';
import { InspectionRowActions } from './RowActions';

// A column says its name on one line: the widths are set for the cells, not for a wrapped head.
const HEADER = 'whitespace-nowrap';

export interface InspectionTableProps {
  inspections: LogInspection[];
  /** The moment the list was read at: times are told from it. */
  nowMicros: number;
  /** The inspection something is being done to, whose controls wait. */
  busyId: string | null;
  /** Why the reader may not manage inspections; undefined when they may. */
  deniedReason: string | undefined;
  emptyLabel: React.ReactNode;
  onToggle: (inspection: LogInspection, enabled: boolean) => void;
  onRun: (inspection: LogInspection) => void;
  onEdit: (inspection: LogInspection) => void;
  onDelete: (inspection: LogInspection) => void;
}

/**
 * The saved inspections of the organization. A row says what is inspected and
 * how often, how the last run went, and when the next one is; the name leads
 * to the reports.
 */
export function InspectionTable({
  inspections,
  nowMicros,
  busyId,
  deniedReason,
  emptyLabel,
  onToggle,
  onRun,
  onEdit,
  onDelete,
}: InspectionTableProps) {
  const { t, i18n } = useTranslation('logs');
  const locale = i18n.resolvedLanguage ?? i18n.language;

  return (
    <DataTable
      rows={inspections}
      rowKey={(inspection) => inspection.id}
      emptyLabel={emptyLabel}
      className="min-w-[1040px] rounded-none border-0 bg-transparent"
      columns={[
        {
          key: 'name',
          header: t('inspections.columns.name'),
          // The one column that gives: it takes the room the others leave and cuts its text to it.
          className: 'w-full max-w-0',
          headerClassName: HEADER,
          cell: (inspection) => (
            <div
              data-log-inspection={inspection.id}
              data-log-inspection-enabled={inspection.enabled}
              className="min-w-0 py-1"
            >
              <Link
                to={`/logs/inspections/${encodeURIComponent(inspection.id)}`}
                className="block truncate rounded-sm font-strong text-tx-0 hover:text-indigo-soft focus-visible:bg-bg-3 focus-visible:outline-none"
                title={inspection.name}
              >
                {inspection.name}
              </Link>
              <div
                className="mt-0.5 truncate text-xs text-tx-2"
                title={inspection.statement || undefined}
              >
                <span className="text-tx-1">{inspection.stream}</span>
                {' · '}
                {inspection.statement.replace(/\s+/g, ' ') || t('explore.inspection.target_all')}
              </div>
            </div>
          ),
        },
        {
          key: 'schedule',
          header: t('inspections.columns.schedule'),
          width: 168,
          headerClassName: HEADER,
          cell: (inspection) => (
            <div>
              <div className="truncate text-tx-1">{scheduleLabel(t, inspection.schedule)}</div>
              <div className="mt-0.5 truncate text-xs text-tx-2">
                {t('inspections.cells.window', { duration: durationLabel(t, inspection.window_secs) })}
              </div>
            </div>
          ),
        },
        {
          key: 'last',
          header: t('inspections.columns.last_run'),
          width: 148,
          headerClassName: HEADER,
          cell: (inspection) => {
            const at = inspection.active_run?.created_at_micros ?? inspection.last_run?.at_micros;

            return (
              <div>
                <StandingMark standing={standingOf(inspection)} />
                {at ? (
                  <div className="mt-1 text-xs text-tx-2" title={formatMicrosActive(at)}>
                    {formatRelativeMicros(at, locale, nowMicros)}
                  </div>
                ) : null}
              </div>
            );
          },
        },
        {
          key: 'next',
          header: t('inspections.columns.next_run'),
          width: 124,
          headerClassName: HEADER,
          cell: (inspection) =>
            inspection.enabled && inspection.next_run_at_micros ? (
              <span className="text-tx-1" title={formatMicrosActive(inspection.next_run_at_micros)}>
                {inspection.next_run_at_micros <= nowMicros
                  ? t('inspections.cells.due')
                  : formatRelativeSpan(inspection.next_run_at_micros, nowMicros, locale)}
              </span>
            ) : (
              <span className="text-tx-3">{t('inspections.cells.paused')}</span>
            ),
        },
        {
          key: 'notify',
          header: t('inspections.columns.notify'),
          width: 148,
          headerClassName: HEADER,
          cell: (inspection) =>
            inspection.notify_routes.length === 0 ? (
              <span className="text-tx-3">{t('inspections.cells.no_channel')}</span>
            ) : (
              <div>
                <div className="truncate text-tx-1">
                  {t(`inspections.notify_when.${inspection.notify_when}`)}
                </div>
                <div className="mt-0.5 text-xs text-tx-2">
                  {t('inspections.cells.channels', { count: inspection.notify_routes.length })}
                </div>
              </div>
            ),
        },
        {
          key: 'enabled',
          header: t('inspections.columns.enabled'),
          width: 64,
          headerClassName: HEADER,
          cell: (inspection) => (
            <Switch
              checked={inspection.enabled}
              disabled={Boolean(deniedReason) || busyId === inspection.id}
              disabledReason={deniedReason}
              aria-label={t('inspections.actions.toggle_named', { name: inspection.name })}
              onCheckedChange={(enabled) => onToggle(inspection, enabled)}
            />
          ),
        },
        {
          key: 'actions',
          header: <span className="sr-only">{t('inspections.columns.actions')}</span>,
          width: 112,
          className: 'overflow-visible text-right',
          cell: (inspection) => (
            <InspectionRowActions
              inspection={inspection}
              busy={busyId === inspection.id}
              deniedReason={deniedReason}
              onRun={() => onRun(inspection)}
              onEdit={() => onEdit(inspection)}
              onDelete={() => onDelete(inspection)}
            />
          ),
        },
      ]}
    />
  );
}
