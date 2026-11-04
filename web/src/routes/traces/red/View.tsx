import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { TraceRedScope } from '@/api/traceRed';
import { useTimeFormatter } from '@/lib/time';
import { EmptyState } from '@/shell/EmptyState';
import { ErrorState } from '@/shell/ErrorState';
import { QueryState } from '@/shell/query/State';
import { useQueryTabSearchParams, useQueryTabState } from '@/shell/queryTabs';
import type { TimeWindow } from '@/stores/useTimeStore';

import type { ParsedTraceStatement, TraceFieldDef, TraceQueryMode } from '../fieldQueryModel';
import { RedBreakdown } from './Breakdown';
import { RedDrillPath } from './DrillPath';
import {
  dimensionFor,
  drillInto,
  drillSteps,
  groupableFields,
  keepSteps,
  narrowDuration,
  narrowToFailures,
  nextDimension,
  type RedSignal,
  withoutStep,
} from './model';
import { type DurationView, RedSignals } from './Signals';
import { type RedRequest, useRedBars, useRedBreakdown, useRedOverview } from './useTraceRed';

interface TraceRedViewProps {
  orgId: string;
  /** The range the query covers, as instants. */
  range: { from: string; to: string };
  /** Goes up when the reader asks for the results again. */
  epoch: number;
  queryMode: TraceQueryMode;
  /** The field query that is applied. */
  queryText: string;
  parsedQuery: ParsedTraceStatement;
  fields: TraceFieldDef[];
  /** Apply another field query: the drill is the query, narrowed a step at a time. */
  onQueryChange: (statement: string) => void;
  /** Apply a field query and list its traces. */
  onOpenSpans: (statement: string) => void;
  onTimeWindowChange: (window: TimeWindow) => void;
  onWidenRange: () => void;
  onClearQuery: () => void;
}

/** A state takes the room the figures would: one surface under the path. */
const STATE_CLASS = 'min-h-[280px] flex-1 rounded-md bg-[var(--functional-surface)]';

/**
 * RED of the requests the query matches: their rate, errors and duration over
 * the range, and the requests broken down by a field. Every step of the drill
 * is a step of the query itself, so the Span and Trace views list exactly the
 * requests in front of the reader, and a link carries the drill with it.
 */
export function TraceRedView({
  orgId,
  range,
  epoch,
  queryMode,
  queryText,
  parsedQuery,
  fields,
  onQueryChange,
  onOpenSpans,
  onTimeWindowChange,
  onWidenRange,
  onClearQuery,
}: TraceRedViewProps) {
  const { t } = useTranslation('traces');
  const { tz } = useTimeFormatter();
  const [searchParams, setSearchParams] = useQueryTabSearchParams();
  const [signal, setSignal] = useQueryTabState<RedSignal>('traces.red.signal', 'rate');
  const [durationView, setDurationView] = useQueryTabState<DurationView>('traces.red.duration', 'trend');
  const scope: TraceRedScope = searchParams.get('scope') === 'all' ? 'all' : 'entry';
  const groupable = React.useMemo(() => groupableFields(fields), [fields]);
  const dimension = dimensionFor(searchParams.get('by'), fields, parsedQuery.filters);
  const steps = React.useMemo(() => drillSteps(queryText), [queryText]);
  const bars = useRedBars(range);
  const request: RedRequest = {
    orgId,
    range,
    q: parsedQuery.q,
    filters: parsedQuery.filters,
    where: parsedQuery.where,
    scope,
    epoch,
    enabled: queryMode === 'fields',
  };
  const overview = useRedOverview(request, bars);
  const breakdown = useRedBreakdown(request, bars, dimension);

  const writeParams = (changes: Record<string, string | null>) => {
    const next = new URLSearchParams(searchParams);
    for (const [key, value] of Object.entries(changes)) {
      if (value === null) next.delete(key);
      else next.set(key, value);
    }
    setSearchParams(next, { replace: true });
  };
  const fieldNamed = (name: string) => fields.find((field) => field.name === name);
  const drill = (value: string) => {
    if (!dimension) return;
    writeParams({ by: nextDimension(dimension, fields, parsedQuery.filters) });
    onQueryChange(drillInto(queryText, fieldNamed(dimension), dimension, value));
  };
  const keep = (count: number) => {
    // Back up the path: the field of the first step left behind is the one to break down by again.
    const left = steps[count]?.clause?.field;
    if (left && groupable.some((field) => field.name === left)) writeParams({ by: left });
    onQueryChange(keepSteps(queryText, count));
  };
  const selectRange = ({ from, to }: { from: number; to: number }) =>
    onTimeWindowChange({
      mode: 'absolute',
      from: new Date(from / 1_000).toISOString(),
      to: new Date(to / 1_000).toISOString(),
    });

  if (queryMode === 'sql') {
    return (
      <div data-red-view className="flex h-full min-h-0 flex-col pb-[12px]">
        <EmptyState
          strategy="query-first"
          className={STATE_CLASS}
          title={t('explore.red.states.sql_title')}
          description={t('explore.red.states.sql_description')}
          primaryAction={{ label: t('explore.red.states.sql_action'), onClick: onClearQuery }}
        />
      </div>
    );
  }

  const path = (
    <RedDrillPath
      steps={steps}
      scope={scope}
      onScopeChange={(next) => writeParams({ scope: next === 'all' ? 'all' : null })}
      onKeep={keep}
      onRemove={(index) => onQueryChange(withoutStep(queryText, index))}
    />
  );

  let content: React.ReactNode;
  const data = overview.data;

  if (overview.isError && !data) {
    content = (
      <ErrorState
        error={overview.error}
        title={t('explore.red.states.failed_title')}
        onRetry={() => void overview.refetch()}
        className={STATE_CLASS}
      />
    );
  } else if (!data) {
    content = <QueryState state="loading" className={STATE_CLASS} />;
  } else if (data.unavailable && data.total.requests === 0) {
    content = (
      <EmptyState
        strategy="query-first"
        className={STATE_CLASS}
        title={t('explore.red.states.unavailable_title')}
        description={t(`explore.red.states.unavailable.${data.unavailable}`)}
        primaryAction={{ label: t('explore.red.states.retry'), onClick: () => void overview.refetch() }}
      />
    );
  } else if (data.total.requests === 0) {
    const entry = scope === 'entry';

    content = (
      <EmptyState
        strategy="query-first"
        className={STATE_CLASS}
        title={t(entry ? 'explore.red.states.empty_entry_title' : 'explore.red.states.empty_title')}
        description={t(entry ? 'explore.red.states.empty_entry_description' : 'explore.red.states.empty_description')}
        primaryAction={
          entry
            ? { label: t('explore.red.states.empty_entry_action'), onClick: () => writeParams({ scope: 'all' }) }
            : { label: t('explore.red.states.empty_action'), onClick: onWidenRange }
        }
      />
    );
  } else {
    content = (
      <>
        <RedSignals
          overview={data}
          bars={bars}
          signal={signal}
          onSignalChange={setSignal}
          durationView={durationView}
          onDurationViewChange={setDurationView}
          failuresOnly={steps.some(
            (step) =>
              step.clause?.field === 'status_code' &&
              step.clause.op === '=' &&
              step.clause.value.toUpperCase() === 'ERROR',
          )}
          stale={overview.isPlaceholderData}
          timezone={tz}
          onRangeSelect={selectRange}
          onFailuresOnly={() => onQueryChange(narrowToFailures(queryText))}
          onDurationSelect={(fromNs, toNs) => onQueryChange(narrowDuration(queryText, fromNs, toNs))}
        />
        <RedBreakdown
          response={breakdown.data}
          pending={breakdown.isFetching}
          error={breakdown.error}
          dimension={dimension}
          fields={groupable}
          total={data.total}
          bars={bars}
          signal={signal}
          onSignalChange={setSignal}
          onDimensionChange={(field) => writeParams({ by: field })}
          onDrill={drill}
          onOpenSpans={(value) =>
            dimension && onOpenSpans(drillInto(queryText, fieldNamed(dimension), dimension, value))
          }
          onRetry={() => void breakdown.refetch()}
        />
      </>
    );
  }

  return (
    // Laid out by its own width, not the window's: half of a split page is as narrow as a phone's.
    <div data-red-view className="flex h-full min-h-0 flex-col gap-[8px] overflow-y-auto pb-[12px] [container-type:inline-size]">
      {path}
      {content}
    </div>
  );
}
