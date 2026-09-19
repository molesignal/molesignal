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
import { formatWindowSummary, useTimeStore } from '@/stores/useTimeStore';

import { formatDurationMs, windowToMicros } from '../_helpers';
import { RumFilterSelect, RumListPage } from '../RumLayout';
import { ALL, initialScope, scopeOptions, type RumScope } from './model';
import { RumOnboarding } from './Onboarding';
import {
  CoreWebVitalsPanel,
  DimensionPanel,
  ExperienceTrend,
  FrequentErrorsPanel,
  SatisfactionPanel,
  SlowPagesPanel,
} from './Panels';

const EMPTY_METRICS: OverviewMetrics = {
  users: 0,
  sessions: 0,
  errorFreeRate: 0,
  lcpP75: 0,
  inpP75: 0,
  clsP75: 0,
};

export function Overview() {
  const { t } = useTranslation('rum');
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
      title={t('title')}
      subtitle={t('subtitle')}
      toolbar={
        <>
          <ChromeButton
            variant={comparePrevious ? 'primary' : 'default'}
            aria-pressed={comparePrevious}
            onClick={() => setComparePrevious((current) => !current)}
          >
            {t('overview.compare_previous')}
          </ChromeButton>
          <TimeRangeChip value={formatWindowSummary(window)} className="border-0" />
          <ChromeButton onClick={refetchAll}>{t('refresh')}</ChromeButton>
        </>
      }
      filterBar={
        <>
          <RumFilterSelect
            label={t('scope.application')}
            value={scope.application}
            options={scopeOptions(
              facets?.applications ?? [],
              t('scope.all_apps'),
            )}
            onChange={(application) =>
              setScope((current) => ({ ...current, application }))
            }
          />
          <RumFilterSelect
            label={t('scope.environment')}
            value={scope.environment}
            options={scopeOptions(
              facets?.environments ?? [],
              t('scope.all_environments'),
            )}
            onChange={(environment) =>
              setScope((current) => ({ ...current, environment }))
            }
          />
          <RumFilterSelect
            label={t('scope.version')}
            value={scope.version}
            options={scopeOptions(
              facets?.versions ?? [],
              t('scope.all_versions'),
            )}
            onChange={(version) =>
              setScope((current) => ({ ...current, version }))
            }
          />
          <RumFilterSelect
            label={t('scope.region')}
            value={scope.country}
            options={scopeOptions(
              facets?.countries ?? [],
              t('scope.all_regions'),
            )}
            onChange={(country) =>
              setScope((current) => ({ ...current, country }))
            }
          />
          <RumFilterSelect
            label={t('scope.device')}
            value={scope.device}
            options={scopeOptions(
              facets?.devices ?? [],
              t('scope.all_devices'),
            )}
            onChange={(device) =>
              setScope((current) => ({ ...current, device }))
            }
          />
        </>
      }
      kpis={
        !currentQuery.isLoading && !error && metrics.sessions > 0
          ? [
              {
                label: t('overview.kpi.active_users'),
                value: metrics.users.toLocaleString(),
                sub: comparisonLabel(
                  metrics.users,
                  previousMetrics.users,
                  comparePrevious,
                  false,
                  t,
                ),
              },
              {
                label: t('overview.kpi.sessions'),
                value: metrics.sessions.toLocaleString(),
                sub: comparisonLabel(
                  metrics.sessions,
                  previousMetrics.sessions,
                  comparePrevious,
                  false,
                  t,
                ),
              },
              {
                label: t('overview.kpi.error_free_sessions'),
                value: formatPercent(metrics.errorFreeRate),
                sub: comparisonLabel(
                  metrics.errorFreeRate,
                  previousMetrics.errorFreeRate,
                  comparePrevious,
                  false,
                  t,
                ),
                tone:
                  metrics.errorFreeRate >= 0.99
                    ? 'good'
                    : metrics.errorFreeRate >= 0.95
                      ? 'warn'
                      : 'danger',
              },
              {
                label: t('overview.kpi.lcp'),
                value: formatVital(metrics.lcpP75),
                sub: comparisonLabel(
                  metrics.lcpP75,
                  previousMetrics.lcpP75,
                  comparePrevious,
                  true,
                  t,
                ),
              },
              {
                label: t('overview.kpi.inp'),
                value: formatVital(metrics.inpP75),
                sub: comparisonLabel(
                  metrics.inpP75,
                  previousMetrics.inpP75,
                  comparePrevious,
                  true,
                  t,
                ),
              },
              {
                label: t('overview.kpi.cls'),
                value: metrics.clsP75 > 0 ? metrics.clsP75.toFixed(3) : '—',
                sub: comparisonLabel(
                  metrics.clsP75,
                  previousMetrics.clsP75,
                  comparePrevious,
                  true,
                  t,
                ),
              },
            ]
          : undefined
      }
      kpiClassName="xl:grid-cols-3 2xl:grid-cols-6"
    >
      {currentQuery.isLoading ? (
        <LoadingState variant="chart" />
      ) : error ? (
        <ErrorState
          title={t('overview.load_error')}
          error={error}
          onRetry={refetchAll}
        />
      ) : metrics.sessions === 0 || !data ? (
        <RumOnboarding />
      ) : (
        <div className="grid gap-[12px] xl:grid-cols-12">
          <div className="xl:col-span-12">
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
          <div className="xl:col-span-8">
            <CoreWebVitalsPanel metrics={metrics} />
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
              rows={data.regions}
            />
          </div>
        </div>
      )}
    </RumListPage>
  );
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

function formatVital(value: number): string {
  return value > 0 ? formatDurationMs(value) : '—';
}

function formatPercent(value: number): string {
  return `${(value * 100).toFixed(2)}%`;
}

function comparisonLabel(
  current: number,
  previous: number,
  enabled: boolean,
  lowerIsBetter: boolean,
  t: ReturnType<typeof useTranslation>['t'],
): string {
  if (!enabled) return t('overview.current_period');
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
