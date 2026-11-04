import type { TFunction } from 'i18next';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

import { formatTraceDurationNs } from '../duration';
import type { CompareRow, MissingCall } from '../model/compare';
import { TraceOperationName } from '../TraceOperationName';

const GRID = 'grid grid-cols-[minmax(200px,1fr)_64px_64px_88px_88px_132px] items-center gap-3';
/** As many lines as a list shows before saying how many more there are. */
const SHOWN = 100;

/** A change of time as a signed word in its colour: more time is worse, less is better. */
export function TimeChange({ deltaNs, baseNs, t }: { deltaNs: number; baseNs: number; t: TFunction<'traces'> }) {
  if (deltaNs === 0) return <span className="text-tx-0">{t('compare.change.none')}</span>;

  const percent = baseNs > 0 ? Math.round((deltaNs / baseNs) * 100) : null;
  return (
    <span className={cn('font-strong tabular-nums', deltaNs > 0 ? 'text-[var(--red-ink)]' : 'text-[var(--green-ink)]')}>
      {deltaNs > 0 ? '+' : '−'}
      {formatTraceDurationNs(Math.abs(deltaNs))}
      <span className="ml-1 font-body text-tx-3">{percent === null ? t('compare.change.new') : `${percent > 0 ? '+' : ''}${percent}%`}</span>
    </span>
  );
}

/**
 * The time the two traces spent on each service, or each operation, side by
 * side, the biggest change first: where the second trace's time went
 * differently. The time is the spans' own work, the time they did not spend
 * waiting on the spans they called.
 */
export function CompareTable({
  rows,
  colors,
  kind,
}: {
  rows: readonly CompareRow[];
  colors: ReadonlyMap<string, string>;
  kind: 'services' | 'operations';
}) {
  const { t } = useTranslation('traces');

  return (
    <div role="table" aria-label={t(`compare.tabs.${kind}`)} data-testid={`trace-compare-${kind}`} className="min-w-[640px]">
      <div role="row" className={cn(GRID, 'sticky top-0 z-10 h-8 border-b border-bd-0 bg-[var(--table-head-surface)] px-3 font-sans text-xs font-strong text-tx-0')}>
        <span role="columnheader">{t(kind === 'services' ? 'compare.columns.service' : 'compare.columns.operation')}</span>
        <span role="columnheader" className="text-right">{t('compare.columns.spans_a')}</span>
        <span role="columnheader" className="text-right">{t('compare.columns.spans_b')}</span>
        <span role="columnheader" className="text-right" title={t('compare.columns.time_hint')}>{t('compare.columns.time_a')}</span>
        <span role="columnheader" className="text-right" title={t('compare.columns.time_hint')}>{t('compare.columns.time_b')}</span>
        <span role="columnheader" className="text-right">{t('compare.columns.change')}</span>
      </div>
      {rows.slice(0, SHOWN).map((row) => (
        <div key={row.key} role="row" data-compare-row={row.key} className={cn(GRID, 'h-8 border-b border-bd-0 px-3 font-sans text-xs')}>
          <span role="cell" className="flex min-w-0 items-center gap-1.5">
            <span aria-hidden="true" className="h-2 w-2 shrink-0 rounded-sm" style={{ backgroundColor: colors.get(row.service) ?? 'var(--chart-1)' }} />
            {row.operation === null ? (
              <span className="min-w-0 truncate text-tx-0" title={row.service}>{row.service}</span>
            ) : (
              <>
                <span className="min-w-0 max-w-[40%] shrink-[2] truncate text-tx-2" title={row.service}>{row.service}</span>
                <TraceOperationName mono={false} operation={row.operation} className="min-w-0 text-tx-0" />
              </>
            )}
          </span>
          <Count value={row.a.spans} />
          <Count value={row.b.spans} />
          <span role="cell" className="text-right tabular-nums text-tx-0">{row.a.spans > 0 ? formatTraceDurationNs(row.a.selfNs) : '—'}</span>
          <span role="cell" className="text-right tabular-nums text-tx-0">{row.b.spans > 0 ? formatTraceDurationNs(row.b.selfNs) : '—'}</span>
          <span role="cell" className="text-right">
            {row.b.spans === 0 ? (
              <span className="text-tx-0">{t('compare.change.gone')}</span>
            ) : (
              <TimeChange deltaNs={row.deltaNs} baseNs={row.a.selfNs} t={t} />
            )}
          </span>
        </div>
      ))}
      <More shown={SHOWN} total={rows.length} t={t} />
    </div>
  );
}

function Count({ value }: { value: number }) {
  return (
    <span role="cell" className="text-right tabular-nums text-tx-0">
      {value > 0 ? value : <span className="text-tx-4">—</span>}
    </span>
  );
}

function More({ shown, total, t }: { shown: number; total: number; t: TFunction<'traces'> }) {
  if (total <= shown) return null;
  return <p className="px-3 py-2 font-sans text-xs text-tx-3">{t('compare.more', { count: total - shown })}</p>;
}

/** The calls one trace made and the other did not, each trace's apart. */
export function MissingCalls({
  onlyInA,
  onlyInB,
  colors,
}: {
  onlyInA: readonly MissingCall[];
  onlyInB: readonly MissingCall[];
  colors: ReadonlyMap<string, string>;
}) {
  const { t } = useTranslation('traces');

  if (onlyInA.length === 0 && onlyInB.length === 0) {
    return <p className="px-3 py-6 text-center font-sans text-xs text-tx-2" data-testid="trace-compare-same-calls">{t('compare.calls.same')}</p>;
  }

  return (
    <div className="grid grid-cols-1 gap-x-6 gap-y-3 px-3 py-2 lg:grid-cols-2" data-testid="trace-compare-calls">
      <CallList side="a" calls={onlyInA} colors={colors} t={t} />
      <CallList side="b" calls={onlyInB} colors={colors} t={t} />
    </div>
  );
}

function CallList({
  side,
  calls,
  colors,
  t,
}: {
  side: 'a' | 'b';
  calls: readonly MissingCall[];
  colors: ReadonlyMap<string, string>;
  t: TFunction<'traces'>;
}) {
  const total = calls.reduce((sum, call) => sum + call.count + call.under, 0);

  return (
    <section aria-label={t(`compare.calls.only_${side}`)} data-testid={`trace-compare-only-${side}`} className="min-w-0">
      <h3 className="flex h-8 items-center gap-2 font-sans text-xs font-strong text-tx-1">
        {t(`compare.calls.only_${side}`)}
        <span className="font-body tabular-nums text-tx-3">{t('compare.calls.spans', { count: total })}</span>
      </h3>
      {calls.length === 0 ? (
        <p className="font-sans text-xs text-tx-3">{t('compare.calls.none')}</p>
      ) : (
        <ul className="flex flex-col">
          {calls.slice(0, SHOWN).map((call) => (
            <MissingCallLine key={call.key} call={call} colors={colors} t={t} />
          ))}
        </ul>
      )}
      <More shown={SHOWN} total={calls.length} t={t} />
    </section>
  );
}

/** The way down to a call, its callers quiet, the call itself said in full. */
const MissingCallLine = React.memo(function MissingCallLine({
  call,
  colors,
  t,
}: {
  call: MissingCall;
  colors: ReadonlyMap<string, string>;
  t: TFunction<'traces'>;
}) {
  const leaf = call.path[call.path.length - 1]!;
  const callers = call.path.slice(0, -1);
  // Past three callers the way down is cut at its top: the nearest callers say where the call was made.
  const shown = callers.slice(-3);
  const full = call.path.map((step) => `${step.service} · ${step.operation}`).join(' › ');

  return (
    <li data-missing-call="" className="flex min-h-8 min-w-0 items-center gap-2 border-b border-bd-0 py-1 font-sans text-xs" title={full}>
      <span className="flex min-w-0 flex-1 items-center gap-1">
        {callers.length > shown.length && <span className="shrink-0 text-tx-4">… ›</span>}
        {shown.map((step, index) => (
          <span key={index} className="flex min-w-0 shrink items-center gap-1 text-tx-3">
            <span className="min-w-0 max-w-[140px] truncate">{step.operation}</span>
            <span aria-hidden="true" className="shrink-0 text-tx-4">›</span>
          </span>
        ))}
        <span aria-hidden="true" className="h-2 w-2 shrink-0 rounded-sm" style={{ backgroundColor: colors.get(leaf.service) ?? 'var(--chart-1)' }} />
        <span className="shrink-0 text-tx-2">{leaf.service}</span>
        <TraceOperationName mono={false} operation={leaf.operation} className="min-w-0 text-tx-0" />
      </span>
      <span className="shrink-0 tabular-nums text-tx-1">×{call.count}</span>
      {call.under > 0 && <span className="shrink-0 tabular-nums text-tx-3">{t('compare.calls.under', { count: call.under })}</span>}
    </li>
  );
});
