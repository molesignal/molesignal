import { ListTree, RefreshCw } from 'lucide-react';
import * as React from 'react';
import { Trans, useTranslation } from 'react-i18next';

import type { TraceRedBreakdownResponse, TraceRedFigures, TraceRedGroup } from '@/api/traceRed';
import { cn } from '@/shell/lib/cn';
import { QueryIconAction } from '@/shell/query/IconAction';
import { Sparkline } from '@/shell/readout/Sparkline';
import { Button } from '@/shell/ui/button';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shell/ui/table';

import type { TraceFieldDef } from '../fieldQueryModel';
import { DimensionSelect } from './DimensionSelect';
import { formatDuration, formatErrorRate, formatRequests } from './format';
import { errorRate, everyBar, pointValue, type RedSignal, sortGroups } from './model';
import type { RedBars } from './useTraceRed';

// Columns a narrow view gives up first: the median and the 99th beside the 95th, then the trend.
const WIDE_ONLY = 'hidden [@container_(min-width:760px)]:table-cell';
const ROOMY_ONLY = 'hidden [@container_(min-width:600px)]:table-cell';

interface RedBreakdownProps {
  /** The breakdown on hand: of the field asked for, or of the one before while it is read. */
  response: TraceRedBreakdownResponse | undefined;
  pending: boolean;
  error: unknown;
  dimension: string | null;
  /** The fields the requests can be broken down by. */
  fields: readonly TraceFieldDef[];
  /** The figures of every request: each group's share is of these. */
  total: TraceRedFigures;
  bars: RedBars;
  signal: RedSignal;
  onSignalChange: (signal: RedSignal) => void;
  onDimensionChange: (field: string) => void;
  /** Narrow the query to the requests with this value and break them down further. */
  onDrill: (value: string) => void;
  /** List the traces of the requests with this value. */
  onOpenSpans: (value: string) => void;
  onRetry: () => void;
}


/**
 * The requests broken down by the values of a field, in the order of the
 * figure in front, each with its share, its rate, errors and duration, and
 * its course over the range. Pressing a value narrows the query to it and
 * goes one field further down.
 */
export function RedBreakdown({
  response,
  pending,
  error,
  dimension,
  fields,
  total,
  bars,
  signal,
  onSignalChange,
  onDimensionChange,
  onDrill,
  onOpenSpans,
  onRetry,
}: RedBreakdownProps) {
  const { t, i18n } = useTranslation('traces');
  const locale = i18n.resolvedLanguage ?? i18n.language;
  // A breakdown of another field is no answer for this one.
  const current = response && response.field === dimension ? response : undefined;
  const groups = React.useMemo(() => sortGroups(current?.groups ?? [], signal), [current?.groups, signal]);
  const share = (group: TraceRedGroup) => (total.requests > 0 ? group.requests / total.requests : 0);

  let body: React.ReactNode;

  if (error && !current) {
    body = (
      <BreakdownNote text={t('explore.red.breakdown.failed')}>
        <Button variant="outline" size="sm" onClick={onRetry}>
          <RefreshCw aria-hidden className="h-3.5 w-3.5" />
          {t('explore.red.states.retry')}
        </Button>
      </BreakdownNote>
    );
  } else if (!current) {
    body = <BreakdownNote text={pending ? t('explore.red.breakdown.loading') : t('explore.red.breakdown.empty')} busy={pending} />;
  } else if (current.unavailable) {
    body = <BreakdownNote text={t(`explore.red.states.unavailable.${current.unavailable}`)} />;
  } else if (groups.length === 0) {
    body = <BreakdownNote text={t('explore.red.breakdown.empty')} />;
  } else {
    body = (
      <Table data-red-breakdown-table containerClassName="overflow-x-auto" className="text-xs">
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead className="w-[38%] pl-4">{t('explore.red.breakdown.value')}</TableHead>
            <SortedHead signal="rate" current={signal} onSort={onSignalChange} label={t('explore.red.breakdown.requests')} />
            <SortedHead signal="errors" current={signal} onSort={onSignalChange} label={t('explore.red.breakdown.error_rate')} />
            <TableHead className={cn('w-[9%] text-right', WIDE_ONLY)}>p50</TableHead>
            <SortedHead signal="duration" current={signal} onSort={onSignalChange} label={t('explore.red.breakdown.p95')} />
            <TableHead className={cn('w-[9%] text-right', WIDE_ONLY)}>p99</TableHead>
            <TableHead className={cn('w-[14%]', ROOMY_ONLY)}>{t('explore.red.breakdown.trend')}</TableHead>
            <TableHead className="w-[44px] pr-3">
              <span className="sr-only">{t('explore.red.breakdown.actions')}</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody className={cn(pending && 'opacity-60')}>
          {groups.map((group) => (
            <BreakdownRow
              key={group.value ?? ''}
              group={group}
              share={share(group)}
              dimension={current.field}
              bars={bars}
              signal={signal}
              locale={locale}
              onDrill={onDrill}
              onOpenSpans={onOpenSpans}
            />
          ))}
        </TableBody>
      </Table>
    );
  }

  return (
    <section data-red-breakdown className="min-w-0 rounded-md bg-[var(--functional-surface)] pb-2">
      <div className="flex min-h-11 flex-wrap items-center gap-x-2 gap-y-1 px-4 py-2 font-sans text-xs text-tx-1">
        <span className="inline-flex items-center gap-1.5 font-medium">
          <Trans
            t={t}
            i18nKey="explore.red.breakdown.by"
            components={{
              field: <DimensionSelect value={dimension} fields={fields} onChange={onDimensionChange} />,
            }}
          />
        </span>
        {current && !current.unavailable && groups.length > 0 && (
          <span data-red-breakdown-count className="text-tx-3">
            {current.more
              ? t('explore.red.breakdown.values_more', { count: groups.length })
              : t('explore.red.breakdown.values', { count: groups.length })}
          </span>
        )}
      </div>
      {body}
    </section>
  );
}

function BreakdownRow({
  group,
  share,
  dimension,
  bars,
  signal,
  locale,
  onDrill,
  onOpenSpans,
}: {
  group: TraceRedGroup;
  share: number;
  dimension: string;
  bars: RedBars;
  signal: RedSignal;
  locale: string;
  onDrill: (value: string) => void;
  onOpenSpans: (value: string) => void;
}) {
  const { t } = useTranslation('traces');
  const failed = errorRate(group);
  const trend = React.useMemo(
    () => everyBar(group.series, bars, (point) => pointValue(point, signal), signal === 'duration' ? null : 0),
    [bars, group.series, signal],
  );
  const value = group.value;

  return (
    <TableRow data-red-group={value ?? ''}>
      <TableCell className="max-w-0 pl-4">
        {value === null ? (
          <span className="text-tx-3">{t('explore.red.breakdown.no_value')}</span>
        ) : (
          <button
            type="button"
            onClick={() => onDrill(value)}
            title={t('explore.red.breakdown.drill', { clause: `${dimension} = ${value}` })}
            className="group block w-full min-w-0 rounded text-left outline-none focus-visible:bg-bg-3"
          >
            <span className="block truncate font-strong text-tx-0 group-hover:text-indigo-soft">{value}</span>
          </button>
        )}
        {/* The group's share of every request: a bar under the value on a track as long as it. */}
        <span aria-hidden="true" className="mt-1 block h-[3px] w-full rounded-full bg-[var(--control-surface)]">
          <span
            data-red-share
            className="block h-full rounded-full bg-blue"
            style={{ width: `${Math.max(share * 100, share > 0 ? 1 : 0)}%` }}
          />
        </span>
      </TableCell>
      <TableCell className="text-right tabular-nums text-tx-0">{formatRequests(group.requests, locale)}</TableCell>
      <TableCell
        className={cn(
          'text-right tabular-nums',
          failed === null || failed === 0 ? 'text-tx-2' : failed >= 0.05 ? 'text-red' : 'text-yellow',
        )}
      >
        {formatErrorRate(failed)}
      </TableCell>
      <TableCell className={cn('text-right tabular-nums text-tx-2', WIDE_ONLY)}>{formatDuration(group.p50Ns)}</TableCell>
      <TableCell className="text-right tabular-nums text-tx-0">{formatDuration(group.p95Ns)}</TableCell>
      <TableCell className={cn('text-right tabular-nums text-tx-2', WIDE_ONLY)}>{formatDuration(group.p99Ns)}</TableCell>
      <TableCell className={ROOMY_ONLY}>
        <Sparkline
          values={trend}
          variant={signal === 'rate' ? 'bars' : 'line'}
          zeroBased={signal !== 'duration'}
          className={cn('h-[22px]', signal === 'errors' ? 'text-red' : signal === 'duration' ? 'text-orange' : 'text-indigo-soft')}
        />
      </TableCell>
      <TableCell className="pr-3">
        {value !== null && (
          <QueryIconAction icon={ListTree} label={t('explore.red.breakdown.open_spans')} onClick={() => onOpenSpans(value)} />
        )}
      </TableCell>
    </TableRow>
  );
}

/** The head of a column the groups can be ordered by: pressing it puts its figure in front. */
function SortedHead({
  signal,
  current,
  onSort,
  label,
}: {
  signal: RedSignal;
  current: RedSignal;
  onSort: (signal: RedSignal) => void;
  label: string;
}) {
  return (
    <TableHead aria-sort={signal === current ? 'descending' : undefined} className="w-[9%] text-right">
      <button
        type="button"
        onClick={() => onSort(signal)}
        className="rounded px-1 outline-none hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
      >
        {label}
      </button>
    </TableHead>
  );
}

function BreakdownNote({ text, busy = false, children }: { text: string; busy?: boolean; children?: React.ReactNode }) {
  return (
    <div
      data-red-breakdown-note
      aria-busy={busy || undefined}
      className="flex min-h-24 flex-col items-center justify-center gap-3 px-4 py-6 text-center font-sans text-xs text-tx-2"
    >
      <span className={cn(busy && 'animate-pulse motion-reduce:animate-none')}>{text}</span>
      {children}
    </div>
  );
}
