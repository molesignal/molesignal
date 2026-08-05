import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as rumApi from '@/api/rum';
import { ChromeButton, TimeRangeChip } from '@/shell/chrome';
import { ErrorState } from '@/shell/ErrorState';
import { LoadingState } from '@/shell/LoadingState';
import { useAuthStore } from '@/stores/auth';
import { formatWindowSummary, useTimeStore } from '@/stores/useTimeStore';

import { formatDurationMs, windowToMicros } from '../_helpers';
import { RumFilterSelect, RumListPage } from '../RumLayout';
import {
  ALL,
  applySessionScope,
  applyVitalScope,
  dimensionShares,
  frequentErrors,
  initialScope,
  overviewMetrics,
  regionShares,
  scopeOptions,
  slowestPages,
  valuesFor,
} from './model';
import { RumOnboarding } from './Onboarding';
import {
  CoreWebVitalsPanel,
  DimensionPanel,
  ExperienceTrend,
  FrequentErrorsPanel,
  SatisfactionPanel,
  SlowPagesPanel,
} from './Panels';

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
  const [comparePrevious, setComparePrevious] = React.useState(true);

  const sessionsQuery = useQuery({
    queryKey: ['rum', 'overview-sessions', orgId, range.from_micros, range.to_micros],
    queryFn: () => rumApi.listSessions({ org_id: orgId, ...range, limit: 500 }),
    enabled: Boolean(orgId),
  });
  const previousSessionsQuery = useQuery({
    queryKey: [
      'rum',
      'overview-sessions-previous',
      orgId,
      previousRange.from_micros,
      previousRange.to_micros,
    ],
    queryFn: () =>
      rumApi.listSessions({ org_id: orgId, ...previousRange, limit: 500 }),
    enabled: Boolean(orgId && comparePrevious),
  });
  const errorsQuery = useQuery({
    queryKey: ['rum', 'overview-errors', orgId, range.from_micros, range.to_micros],
    queryFn: () => rumApi.listErrors({ org_id: orgId, ...range, limit: 20 }),
    enabled: Boolean(orgId),
  });
  const vitalsQuery = useQuery({
    queryKey: ['rum', 'overview-vitals', orgId, range.from_micros, range.to_micros],
    queryFn: () =>
      rumApi.webVitalsSeries({ org_id: orgId, ...range, limit: 1_000 }),
    enabled: Boolean(orgId),
  });
  const previousVitalsQuery = useQuery({
    queryKey: [
      'rum',
      'overview-vitals-previous',
      orgId,
      previousRange.from_micros,
      previousRange.to_micros,
    ],
    queryFn: () =>
      rumApi.webVitalsSeries({
        org_id: orgId,
        ...previousRange,
        limit: 1_000,
      }),
    enabled: Boolean(orgId && comparePrevious),
  });

  const allSessions = sessionsQuery.data?.items ?? [];
  const sessions = applySessionScope(allSessions, scope);
  const previousSessions = applySessionScope(
    previousSessionsQuery.data?.items ?? [],
    scope,
  );
  const vitals = applyVitalScope(vitalsQuery.data ?? [], scope);
  const previousVitals = applyVitalScope(
    previousVitalsQuery.data ?? [],
    scope,
  );
  const metrics = overviewMetrics(sessions, vitals);
  const previousMetrics = overviewMetrics(previousSessions, previousVitals);
  const pages = slowestPages(vitals, sessions);
  const errors = frequentErrors(
    (errorsQuery.data?.items ?? []).filter(
      (error) => scope.version === ALL || error.version === scope.version,
    ),
  );
  const isLoading =
    sessionsQuery.isLoading || errorsQuery.isLoading || vitalsQuery.isLoading;
  const error = sessionsQuery.error ?? errorsQuery.error ?? vitalsQuery.error;

  const refetchAll = () => {
    void Promise.all([
      sessionsQuery.refetch(),
      errorsQuery.refetch(),
      vitalsQuery.refetch(),
      comparePrevious ? previousSessionsQuery.refetch() : Promise.resolve(),
      comparePrevious ? previousVitalsQuery.refetch() : Promise.resolve(),
    ]);
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
          <TimeRangeChip value={formatWindowSummary(window)} />
          <ChromeButton onClick={refetchAll}>{t('refresh')}</ChromeButton>
        </>
      }
      filterBar={
        <>
          <RumFilterSelect
            label={t('scope.application')}
            value={scope.application}
            options={scopeOptions(
              valuesFor(allSessions, 'application'),
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
              valuesFor(allSessions, 'environment'),
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
              valuesFor(allSessions, 'version'),
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
              valuesFor(allSessions, 'country'),
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
              valuesFor(allSessions, 'device'),
              t('scope.all_devices'),
            )}
            onChange={(device) =>
              setScope((current) => ({ ...current, device }))
            }
          />
        </>
      }
      kpis={
        !isLoading && !error && sessions.length > 0
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
      {isLoading ? (
        <LoadingState variant="chart" />
      ) : error ? (
        <ErrorState
          title={t('overview.load_error')}
          error={error}
          onRetry={refetchAll}
        />
      ) : sessions.length === 0 ? (
        <RumOnboarding />
      ) : (
        <div className="grid gap-6 xl:grid-cols-12">
          <div className="xl:col-span-12">
            <ExperienceTrend sessions={sessions} range={range} />
          </div>
          <div className="xl:col-span-8">
            <CoreWebVitalsPanel metrics={metrics} />
          </div>
          <div className="xl:col-span-4">
            <SatisfactionPanel sessions={sessions} />
          </div>
          <div className="xl:col-span-7">
            <SlowPagesPanel pages={pages} />
          </div>
          <div className="xl:col-span-5">
            <FrequentErrorsPanel errors={errors} />
          </div>
          <div className="xl:col-span-7">
            <DimensionPanel
              title={t('overview.browser_device')}
              description={t('overview.browser_device_description')}
              rows={dimensionShares(sessions, ['browser', 'device'])}
            />
          </div>
          <div className="xl:col-span-5">
            <DimensionPanel
              title={t('overview.regions')}
              description={t('overview.regions_description')}
              rows={regionShares(sessions)}
            />
          </div>
        </div>
      )}
    </RumListPage>
  );
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
