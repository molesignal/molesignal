import { useTranslation } from 'react-i18next';

import type { SloStatus } from '@/api/apmSlo';
import { cn } from '@/shell/lib/cn';
import { LONG_LIST_ROW_CLASS, longListRowStyle } from '@/shell/list/longList';
import { TraceOperationName } from '@/viz/trace/TraceOperationName';

import { formatBudget, formatBurn, formatSli, SLO_STATE_FILL, SLO_STATE_TEXT, SLO_STATE_WEIGHT, sloTarget } from './model';
import { gridMinWidth } from '../frame/gridWidth';

const TRACKS = ['minmax(240px,1fr)', '120px', '112px', '84px', '148px', '64px', '64px', '72px'];
const ROW_STYLE = { height: 40, ...longListRowStyle(39) };

/**
 * The objectives, a row each: what each promises and of which service, what
 * it has met over its window, how much of its budget is left, how fast it is
 * spending it, and so how it stands. A row opens the objective.
 */
export function SloTable({ statuses, onOpen }: { statuses: readonly SloStatus[]; onOpen: (id: string) => void }) {
  const { t } = useTranslation('apm');
  const grid = TRACKS.join(' ');
  const minWidth = gridMinWidth(TRACKS);
  const heads = ['name', 'target', 'objective', 'sli', 'budget', 'burn_1h', 'burn_6h', 'state'] as const;

  return (
    <div role="table" aria-label={t('slo.table')} data-apm-slo-table="" className="overflow-x-auto">
      <div role="row" className="sticky top-0 grid h-8 items-center gap-3 border-b border-bd-0 bg-[var(--table-head-surface)] px-3 font-sans text-xs font-strong text-tx-0" style={{ gridTemplateColumns: grid, minWidth }}>
        {heads.map((head) => (
          <span key={head} role="columnheader" className={cn('truncate', head !== 'name' && head !== 'target' && head !== 'budget' && 'text-right')}>
            {t(`slo.columns.${head}`)}
          </span>
        ))}
      </div>
      {statuses.map((status) => {
        const { slo } = status;
        const remaining = status.budgetRemaining ?? null;

        return (
          <button
            key={slo.id}
            type="button"
            role="row"
            data-apm-slo-row={slo.id}
            onClick={() => onOpen(slo.id)}
            style={{ ...ROW_STYLE, gridTemplateColumns: grid, minWidth }}
            className={cn('grid w-full items-center gap-3 border-b border-[var(--row-rule)] px-3 text-left font-sans text-xs outline-none hover:bg-bg-2 focus-visible:bg-bg-2', LONG_LIST_ROW_CLASS)}
          >
            <span role="cell" className="min-w-0">
              <span className="block truncate font-strong text-tx-0" title={slo.name}>
                {slo.name}
              </span>
              <span className="flex min-w-0 items-center gap-1.5 text-tx-3">
                <span className="truncate">
                  {slo.service} · {slo.environment}
                </span>
                {slo.transaction && <TraceOperationName mono={false} operation={slo.transaction.name} className="min-w-0 text-tx-2" />}
              </span>
            </span>
            <span role="cell" className="truncate text-tx-0">
              {sloTarget(slo, t)}
            </span>
            <span role="cell" className="text-right tabular-nums text-tx-0">
              {formatSli(slo.objective)} · {t('slo.window', { count: slo.windowDays })}
            </span>
            <span role="cell" className={cn('text-right tabular-nums', SLO_STATE_WEIGHT[status.state], SLO_STATE_TEXT[status.state])}>
              {formatSli(status.sli)}
            </span>
            <span role="cell" className="flex min-w-0 items-center gap-2">
              <span aria-hidden className="relative h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-bg-3">
                <span
                  className="absolute inset-y-0 left-0 rounded-full"
                  style={{ width: `${Math.max(0, Math.min(1, remaining ?? 0)) * 100}%`, background: SLO_STATE_FILL[status.state] }}
                />
              </span>
              <span className="w-10 shrink-0 text-right tabular-nums text-tx-0">{formatBudget(remaining)}</span>
            </span>
            <span role="cell" className="text-right tabular-nums text-tx-0">
              {formatBurn(status.burnRates.oneHour)}
            </span>
            <span role="cell" className="text-right tabular-nums text-tx-0">
              {formatBurn(status.burnRates.sixHours)}
            </span>
            <span role="cell" data-apm-slo-state={status.state} className={cn('text-right', SLO_STATE_WEIGHT[status.state], SLO_STATE_TEXT[status.state])}>
              {t(`slo.state.${status.state}`)}
            </span>
          </button>
        );
      })}
    </div>
  );
}
