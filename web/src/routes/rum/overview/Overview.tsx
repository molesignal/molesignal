import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import {
  getOverview,
  getOverviewInsights,
  type OverviewMetrics,
  type OverviewParams,
} from '@/api/rum/overview';
import { ChromeButton, TimeRangeChip } from '@/shell/chrome';
import { ErrorState } from '@/shell/ErrorState';
import { LoadingState } from '@/shell/LoadingState';
import { useAuthStore } from '@/stores/auth';
import { useTimeStore } from '@/stores/useTimeStore';

import { regionName, windowToMicros } from '../_helpers';
import { RumFilterSelect, RumListPage } from '../RumLayout';
import { ALL, initialScope, scopeOptions, type RumScope } from './model';
import { RumOnboarding } from './Onboarding';
import {
  DimensionPanel,
  ExperienceTrend,
  FrequentErrorsPanel,
  SatisfactionPanel,
  SlowPagesPanel,
} from './Panels';
import { RumPulse } from './Pulse';

const EMPTY_METRICS: OverviewMetrics = {
  users: 0,
  sessions: 0,
  errorFreeRate: 0,
  lcpP75: 0,
  inpP75: 0,
  clsP75: 0,
};

export function Overview() {
  const { t, i18n } = useTranslation('rum');
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const window = useTimeStore((state) => state.window);
  const range = React.useMemo(() => windowToMicros(window), [window]);
  const previousRange = React.useMemo(() => {
    const duration = Math.max(1, range.to_micros - range.from_micros);
    return {
      from_micros: range.from_micros - duration,
      to_micros: range.from_micros,
    };
  }, [range]);
  const [scope, setScope] = React.useState(initialScope);
  const [comparePrevious, setComparePrevious] = React.useState(false);
  const requestScope = React.useMemo(() => overviewScope(scope), [scope]);
  const hasScope = Object.keys(requestScope).length > 0;

  const baseQuery = useQuery({
    queryKey: [
      'rum',
      'overview',
      'base',
      orgId,
      range.from_micros,
      range.to_micros,
    ],
    queryFn: () => getOverview({ org_id: orgId, ...range }),
    enabled: Boolean(orgId),
  });
  const scopedQuery = useQuery({
    queryKey: [
      'rum',
      'overview',
      'scope',
      orgId,
      range.from_micros,
      range.to_micros,
      scope.application,
      scope.environment,
      scope.version,
      scope.country,
      scope.device,
    ],
    queryFn: () =>
      getOverview({ org_id: orgId, ...range, ...requestScope }),
    enabled: Boolean(orgId) && hasScope,
  });
  const currentQuery = hasScope ? scopedQuery : baseQuery;
  const insightsQuery = useQuery({
    queryKey: [
      'rum',
      'overview',
      'insights',
      orgId,
      range.from_micros,
      range.to_micros,
      scope.application,
      scope.environment,
      scope.version,
      scope.country,
      scope.device,
    ],
    queryFn: () =>
      getOverviewInsights({ org_id: orgId, ...range, ...requestScope }),
    enabled:
      Boolean(orgId) &&
      currentQuery.isSuccess &&
      (currentQuery.data?.metrics.sessions ?? 0) > 0,
  });
  const previousQuery = useQuery({
    queryKey: [
      'rum',
      'overview',
      'previous',
      orgId,
      previousRange.from_micros,
      previousRange.to_micros,
      scope.application,
      scope.environment,
      scope.version,
      scope.country,
      scope.device,
    ],
    queryFn: () =>
      getOverview({
        org_id: orgId,
        ...previousRange,
        ...requestScope,
        summary_only: true,
      }),
    enabled:
      Boolean(orgId) && comparePrevious && currentQuery.isSuccess,
  });

  const data = currentQuery.data;
  const insights = insightsQuery.data;
  const metrics = data?.metrics ?? EMPTY_METRICS;
  const previousMetrics = previousQuery.data?.metrics ?? EMPTY_METRICS;
  const facets = baseQuery.data?.facets;
  const error = currentQuery.error;

  // Whether sessions reach this workspace at all, judged before any narrowing and held while
  // the next window loads, so the scope row neither shows up for a page that has nothing to
  // narrow nor blinks out on every range change.
  const workspaceSessions = baseQuery.data?.metrics.sessions;
  const collecting = useLastDefined(
    workspaceSessions === undefined ? undefined : workspaceSessions > 0,
  );

  const settled = !currentQuery.isLoading && !error;
  // Sessions exist but none in this slice: say so and keep the way back, instead of reading as
  // "the SDK is not sending yet".
  const scopedToNothing =
    settled && hasScope && metrics.sessions === 0 && collecting === true;
  const onboarding = settled && metrics.sessions === 0 && !scopedToNothing;

  const refetchAll = () => {
    void (async () => {
      const summaryRequests = [baseQuery.refetch()];
      if (hasScope) summaryRequests.push(scopedQuery.refetch());
      const summaryResults = await Promise.all(summaryRequests);
      const refreshedCurrent = hasScope ? summaryResults[1] : summaryResults[0];

      const deferredRequests: Array<Promise<unknown>> = [];
      if ((refreshedCurrent?.data?.metrics.sessions ?? 0) > 0) {
        deferredRequests.push(insightsQuery.refetch());
      }
      if (comparePrevious) deferredRequests.push(previousQuery.refetch());
      await Promise.all(deferredRequests);
    })();
  };

  return (
    <RumListPage
      toolbar={
        <>
          <ChromeButton
            variant={comparePrevious ? 'primary' : 'default'}
            aria-pressed={comparePrevious}
            onClick={() => setComparePrevious((current) => !current)}
          >
            {t('overview.compare_previous')}
          </ChromeButton>
          <TimeRangeChip className="border-0" />
          <ChromeButton onClick={refetchAll}>{t('refresh')}</ChromeButton>
        </>
      }
      // Nothing to scope before the first session arrives; a scope in force stays reachable.
      scope={
        hasScope || collecting ? (
          <>
            {SCOPE_SELECTS.map(({ key, facet, label, all }) => (
              <RumFilterSelect
                key={key}
                label={t(label)}
                value={scope[key]}
                options={scopeOptions(
                  withSelected(facets?.[facet] ?? [], scope[key]),
                  t(all),
                )}
                onChange={(value) => setScope((current) => ({ ...current, [key]: value }))}
              />
            ))}
          </>
        ) : undefined
      }
      state={
        scopedToNothing
          ? {
              variant: 'empty',
              title: t('overview.scope_empty_title'),
              description: t('overview.scope_empty_description'),
              action: (
                <ChromeButton onClick={() => setScope(initialScope())}>
                  {t('overview.scope_clear')}
                </ChromeButton>
              ),
            }
          : null
      }
    >
      {currentQuery.isLoading ? (
        <LoadingState variant="chart" />
      ) : error ? (
        <ErrorState
          title={t('overview.load_error')}
          error={error}
          onRetry={refetchAll}
        />
      ) : onboarding || !data ? (
        <RumOnboarding />
      ) : (
        <div className="space-y-[12px]">
          <RumPulse
            metrics={metrics}
            trend={insights?.trend}
            comparison={
              comparePrevious
                ? {
                    users: comparisonLabel(metrics.users, previousMetrics.users, false, t),
                    sessions: comparisonLabel(metrics.sessions, previousMetrics.sessions, false, t),
                    errorFreeRate: comparisonLabel(
                      metrics.errorFreeRate,
                      previousMetrics.errorFreeRate,
                      false,
                      t,
                    ),
                    lcpP75: comparisonLabel(metrics.lcpP75, previousMetrics.lcpP75, true, t),
                    inpP75: comparisonLabel(metrics.inpP75, previousMetrics.inpP75, true, t),
                    clsP75: comparisonLabel(metrics.clsP75, previousMetrics.clsP75, true, t),
                  }
                : undefined
            }
          />
          <div className="grid gap-[12px] xl:grid-cols-12">
            <div className={insights ? 'xl:col-span-8' : 'xl:col-span-12'}>
              {insights ? (
                <ExperienceTrend buckets={insights.trend} range={range} />
              ) : insightsQuery.error ? (
                <ErrorState
                  title={t('overview.load_error')}
                  error={insightsQuery.error}
                  onRetry={() => void insightsQuery.refetch()}
                />
              ) : (
                <LoadingState variant="chart" />
              )}
            </div>
            {insights ? (
              <>
                <div className="xl:col-span-4">
                  <SatisfactionPanel counts={insights.satisfaction} />
                </div>
                <div className="xl:col-span-7">
                  <SlowPagesPanel pages={insights.slowPages} />
                </div>
                <div className="xl:col-span-5">
                  <FrequentErrorsPanel errors={insights.frequentErrors} />
                </div>
              </>
            ) : null}
            <div className="xl:col-span-7">
              <DimensionPanel
                title={t('overview.browser_device')}
                description={t('overview.browser_device_description')}
                rows={data.browserDevices}
              />
            </div>
            <div className="xl:col-span-5">
              <DimensionPanel
                title={t('overview.regions')}
                description={t('overview.regions_description')}
                // A region is read by its name; its code is what it is filtered by.
                rows={data.regions.map((row) => ({
                  ...row,
                  label: regionName(row.label, i18n.resolvedLanguage ?? i18n.language),
                }))}
              />
            </div>
          </div>
        </div>
      )}
    </RumListPage>
  );
}

// The dimensions the overview can be narrowed to, in the order they are offered.
const SCOPE_SELECTS = [
  { key: 'application', facet: 'applications', label: 'scope.application', all: 'scope.all_apps' },
  { key: 'environment', facet: 'environments', label: 'scope.environment', all: 'scope.all_environments' },
  { key: 'version', facet: 'versions', label: 'scope.version', all: 'scope.all_versions' },
  { key: 'country', facet: 'countries', label: 'scope.region', all: 'scope.all_regions' },
  { key: 'device', facet: 'devices', label: 'scope.device', all: 'scope.all_devices' },
] as const;

/** The facet values with the value in force among them, so the select can always show it. */
function withSelected(values: string[], selected: string): string[] {
  return selected === ALL || values.includes(selected) ? values : [selected, ...values];
}

/** The last answer that was not `undefined`: a value rides out the gap while its query reloads. */
function useLastDefined<T>(value: T | undefined): T | undefined {
  const [last, setLast] = React.useState(value);
  if (value !== undefined && value !== last) setLast(value);
  return value ?? last;
}

function overviewScope(scope: RumScope): Partial<OverviewParams> {
  return {
    ...(scope.application !== ALL ? { application: scope.application } : {}),
    ...(scope.environment !== ALL ? { environment: scope.environment } : {}),
    ...(scope.version !== ALL ? { version: scope.version } : {}),
    ...(scope.country !== ALL ? { country: scope.country } : {}),
    ...(scope.device !== ALL ? { device: scope.device } : {}),
  };
}

/** How a figure compares with the period before, in words. */
function comparisonLabel(
  current: number,
  previous: number,
  lowerIsBetter: boolean,
  t: ReturnType<typeof useTranslation>['t'],
): string {
  if (!Number.isFinite(previous) || previous === 0) {
    return t('overview.no_previous_data');
  }
  const delta = ((current - previous) / Math.abs(previous)) * 100;
  if (Math.abs(delta) < 0.05) return t('overview.unchanged');
  const improving = lowerIsBetter ? delta < 0 : delta > 0;
  return t(improving ? 'overview.delta_better' : 'overview.delta_worse', {
    value: `${Math.abs(delta).toFixed(1)}%`,
  });
}
