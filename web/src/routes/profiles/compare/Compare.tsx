import { useQuery } from '@tanstack/react-query';
import { RefreshCw } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useSearchParams } from 'react-router-dom';

import * as profilesApi from '@/api/profiles';
import type { ProfileTypeName } from '@/api/profiles';
import { ListPage } from '@/product/templates';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { QueryState } from '@/shell/query/State';
import { useTimeStore } from '@/stores/useTimeStore';
import { TimeRangeControl } from '@/time/TimePicker';
import { diffFunctions, diffTotals } from '@/viz/profiles/flamebearer';
import { Flamegraph } from '@/viz/profiles/Flamegraph';

import { profileTypesIn, ServiceSelect, TruncatedNotice, TypeSelect } from '../shared';
import { BaselinePicker, timeRangeLabel } from './BaselinePicker';
import { ChangedFunctions } from './ChangedFunctions';
import { type CompareBaseline, comparePeriods, settledBaseline, windowPeriod } from './periods';
import { CompareStatement } from './Statement';

/** As many profiles as are listed to learn which services and types the periods hold. */
const SCOPE_LIMIT = 500;

/** What tells one baseline from another in a query key. */
function baselineKey(baseline: CompareBaseline): string {
  return baseline.kind === 'window'
    ? `window:${baseline.window.from}:${baseline.window.to}`
    : baseline.kind;
}

/**
 * What got hotter or cooler in one stretch of time against another. Both are
 * set freely, with the time range every page has: the period looked at, and
 * the baseline, which may also follow the period as the stretch before it or
 * the same stretch a day or a week earlier.
 *
 * The page compares as soon as it is opened and again when what is compared
 * changes: every control has a value to start from, so there is nothing to
 * fill in first.
 */
export function ProfilesCompare() {
  const { t } = useTranslation('profiles');
  const [searchParams] = useSearchParams();
  const timeWindow = useTimeStore((state) => state.window);
  const [service, setService] = React.useState(() => searchParams.get('service') ?? '');
  // The type the reader picked or arrived with. Until then, the first type the periods hold.
  const [chosenType, setChosenType] = React.useState(() => searchParams.get('type') ?? '');
  const [chosenBaseline, setChosenBaseline] = React.useState<CompareBaseline>({ kind: 'previous' });
  const [selectedFunction, setSelectedFunction] = React.useState<string | null>(null);

  const winKey = [timeWindow.from, timeWindow.to] as const;
  // The spans as they stand while the comparison is asked for; its answer brings the exact ones.
  const { t: tCommon } = useTranslation('common');
  const pending = React.useMemo(
    () => comparePeriods(windowPeriod(timeWindow), chosenBaseline),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the window is its two expressions
    [timeWindow.from, timeWindow.to, chosenBaseline],
  );
  const baseline = settledBaseline(chosenBaseline, pending.comparison);
  const baselineId = baselineKey(baseline);

  const scopeQuery = useQuery({
    queryKey: ['profiles-compare-scope', ...winKey, baselineId],
    queryFn: () => {
      const periods = comparePeriods(windowPeriod(timeWindow), baseline);

      // Both stretches and whatever lies between them: a baseline of one's own may be either side.
      return profilesApi.list({
        from: Math.min(periods.baseline.from, periods.comparison.from),
        to: Math.max(periods.baseline.to, periods.comparison.to),
        limit: SCOPE_LIMIT,
      });
    },
  });
  const scopeRows = React.useMemo(() => scopeQuery.data ?? [], [scopeQuery.data]);
  const presentTypes = React.useMemo(() => profileTypesIn(scopeRows), [scopeRows]);
  const services = React.useMemo(
    () => Array.from(new Set(scopeRows.map((row) => row.service).filter(Boolean))).sort(),
    [scopeRows],
  );
  const type = chosenType || presentTypes[0] || 'cpu';

  const diffQuery = useQuery({
    queryKey: ['profiles-diff', ...winKey, baselineId, service, type],
    // Which type to compare is not known before the periods have been looked into.
    enabled: chosenType !== '' || !scopeQuery.isLoading,
    queryFn: async () => {
      const periods = comparePeriods(windowPeriod(timeWindow), baseline);
      const result = await profilesApi.diff({
        service: service || undefined,
        type: type as ProfileTypeName,
        from: periods.comparison.from,
        to: periods.comparison.to,
        baseline_from: periods.baseline.from,
        baseline_to: periods.baseline.to,
      });

      return { ...result, periods };
    },
  });

  React.useEffect(() => {
    setSelectedFunction(null);
  }, [baselineId, service, timeWindow.from, timeWindow.to, type]);

  const data = diffQuery.data;
  const fb = data?.flamebearer;
  const periods = data?.periods ?? pending;
  const totals = React.useMemo(() => (fb ? diffTotals(fb) : undefined), [fb]);
  const changed = React.useMemo(() => (fb ? diffFunctions(fb) : []), [fb]);
  const loading = diffQuery.isLoading || (chosenType === '' && scopeQuery.isLoading);
  const missing: 'none' | 'baseline' | 'comparison' | null = !data
    ? null
    : data.baseline_count === 0 && data.comparison_count === 0
      ? 'none'
      : data.baseline_count === 0
        ? 'baseline'
        : data.comparison_count === 0
          ? 'comparison'
          : null;

  const refresh = () => {
    void scopeQuery.refetch();
    void diffQuery.refetch();
  };

  const filters = (
    <div
      role="group"
      aria-label={t('compare.filters_aria')}
      className="flex min-w-0 flex-1 flex-wrap items-center gap-2"
    >
      <ServiceSelect value={service} services={services} onChange={setService} />
      <TypeSelect
        value={type}
        onChange={setChosenType}
        present={scopeRows.length < SCOPE_LIMIT ? presentTypes : undefined}
      />
      <ChromeButton size="sm" className="ml-auto" onClick={refresh}>
        <RefreshCw
          className={cn('h-3.5 w-3.5', (diffQuery.isFetching || scopeQuery.isFetching) && 'animate-spin')}
        />
        {t('refresh')}
      </ChromeButton>
    </div>
  );

  return (
    <ListPage
      appearance="surface"
      title={t('compare.title')}
      subtitle={t('compare.subtitle') as string}
      filters={filters}
      breadcrumbs={[
        { labelKey: 'profiles', to: '/profiles' },
        { labelKey: 'breadcrumbs.profiles_compare' },
      ]}
      // The page is several surfaces of its own: they sit on the canvas, not in one more card.
      contentSurface={false}
    >
      <div className="space-y-[12px]">
        <CompareStatement
          comparison={periods.comparison}
          baseline={periods.baseline}
          // The two periods are chosen where they are said, side by side: the time range every
          // page has, and the baseline's own.
          comparisonControl={
            // The times a range spans are said beside the control, in the reader's time zone:
            // the control names the range, as the baseline's does.
            <TimeRangeControl fill align="start" value={timeRangeLabel(timeWindow, t, tCommon)} />
          }
          baselineControl={
            <BaselinePicker
              value={baseline}
              comparison={periods.comparison}
              resolved={periods.baseline}
              onChange={setChosenBaseline}
            />
          }
          counts={data ? { comparison: data.comparison_count, baseline: data.baseline_count } : undefined}
          totals={totals}
          units={fb?.units}
        />

        {loading || diffQuery.isError || missing ? (
          <section className="overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]">
            {loading ? (
              <QueryState state="loading" loadingLabel={t('compare.loading')} className="min-h-[320px]" />
            ) : diffQuery.isError ? (
              <QueryState state="error" error={diffQuery.error} className="min-h-[320px]" />
            ) : (
              <QueryState
                state="empty"
                empty={
                  <div data-compare-missing={missing}>
                    <div className="font-semibold text-tx-1">{t(`compare.empty.${missing}_title`)}</div>
                    <div className="mx-auto mt-1 max-w-[520px] text-xs text-tx-3">
                      {t(`compare.empty.${missing}_description`)}
                    </div>
                    {missing === 'baseline' ? (
                      // The period has profiles of its own: they can be read without a baseline.
                      <Link
                        to="/profiles"
                        className="mt-3 inline-flex h-8 items-center rounded-md bg-bg-2 px-2.5 font-sans text-xs font-strong text-tx-1 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
                      >
                        {t('compare.empty.open_analysis')}
                      </Link>
                    ) : null}
                  </div>
                }
                className="min-h-[320px]"
              />
            )}
          </section>
        ) : fb ? (
          <>
            {data.truncated && <TruncatedNotice />}
            <ChangedFunctions
              rows={changed}
              units={fb.units}
              selected={selectedFunction}
              onSelect={setSelectedFunction}
            />
            <Flamegraph
              flamebearer={fb}
              diff
              selectedFunction={selectedFunction}
              onSelectedFunctionChange={setSelectedFunction}
              service={service || undefined}
              className="border-0 [box-shadow:var(--shadow-functional-surface)]"
            />
          </>
        ) : null}
      </div>
    </ListPage>
  );
}
