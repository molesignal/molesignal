import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import { apmApi, apmQueryKeys } from '@/api/apm';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { RedCharts } from '../charts/RedCharts';
import { ApmFirstRun } from '../frame/Activation';
import { ApmNavigation } from '../frame/Navigation';
import { ApmPage, ApmQueryState } from '../frame/Page';
import { ApmTimeControls } from '../frame/TimeControls';
import { useApmScope } from '../frame/useApmScope';
import { InsightsCard } from '../insights/InsightsCard';
import { useInsights } from '../insights/useInsights';
import { apmQueryParams, compareEnvironment, compareShiftMicros } from '../scope';
import { HealthCard } from './HealthCard';
import { EnvironmentsCard, InstrumentationCard } from './Landscape';
import { OverviewPulse } from './Pulse';
import { RankingsCard } from './Rankings';
import { ActiveAlertsCard, DeploymentsCard } from './Signals';

const ALL = '__all__';

/**
 * APM's landing page: all the services together over the time in view. How
 * much they served, how often they failed and how long they took; how their
 * health divides and moved; which weigh most; what insights find; the
 * alerts going on, what was deployed, where they run and what they report.
 */
export function ApmOverview() {
  const { t } = useTranslation('apm');
  const { scope, setScope, range, compareRange, orgId, searchParams } = useApmScope();
  const params = apmQueryParams(scope, range);
  // Every service together has no other environment to be compared with.
  const compareParams = compareRange && !compareEnvironment(scope.compare) ? apmQueryParams(scope, compareRange) : null;

  const overview = useQuery({
    queryKey: apmQueryKeys.overview(orgId, params),
    queryFn: () => apmApi.overview(params),
    enabled: Boolean(orgId),
    refetchInterval: 60_000,
  });
  const insights = useInsights(orgId, params);
  const compared = useQuery({
    queryKey: apmQueryKeys.overview(orgId, compareParams ?? {}),
    queryFn: () => apmApi.overview(compareParams ?? {}),
    enabled: Boolean(orgId && compareParams),
  });

  const data = overview.data;
  const environments = data?.availableEnvironments ?? [];
  const chartRange = data ? { from: data.meta.range.from, to: data.meta.range.to } : { from: params.from ?? 0, to: params.to ?? 0 };

  return (
    <ApmPage
      page="overview"
      title={t('title')}
      subtitle={t('subtitle')}
      breadcrumbs={null}
      navigation={
        <ApmNavigation end={<ApmTimeControls scope={scope} onScopeChange={setScope} comparable />}>
          <FittedSelect
            aria-label={t('catalog.environment')}
            value={scope.environment || ALL}
            onValueChange={(value) => setScope({ environment: value === ALL ? '' : value })}
            options={[{ value: ALL, label: t('catalog.all_environments') }, ...environments.map((value) => ({ value, label: value }))]}
            maxWidth={200}
            prefix={scope.environment ? t('catalog.environment') : undefined}
            className="h-8 border-0 bg-[var(--control-surface)] px-2.5 text-xs font-strong text-tx-1 hover:bg-bg-3"
          />
        </ApmNavigation>
      }
    >
      <ApmQueryState query={overview} empty={false}>
        {data &&
          ((data.serviceCount ?? data.services.length) === 0 ? (
            <ApmFirstRun />
          ) : (
            <div data-apm-overview="" className="space-y-[12px]">
              <OverviewPulse data={data} range={range} />
              <RedCharts
                trend={data.trend}
                compared={compareParams ? compared.data?.trend : null}
                shiftMicros={compareShiftMicros(scope.compare)}
                resolution={data.meta.resolution}
                versions={[]}
                range={chartRange}
              />
              <div className="grid min-w-0 gap-[12px] xl:grid-cols-2">
                <HealthCard counts={data.serviceHealth} trend={data.healthTrend ?? []} range={chartRange} />
                {insights.data && (
                  // A version that did worse, the deployments card says.
                  <InsightsCard data={insights.data} range={range} hide={['regression']} />
                )}
              </div>
              {data.rankings && <RankingsCard rankings={data.rankings} range={range} searchParams={searchParams} />}
              <div className="grid min-w-0 gap-[12px] xl:grid-cols-2">
                <ActiveAlertsCard />
                <DeploymentsCard deployments={data.deployments ?? []} searchParams={searchParams} />
              </div>
              <div className="grid min-w-0 gap-[12px] xl:grid-cols-2">
                <EnvironmentsCard environments={data.environments ?? []} />
                {data.instrumentation && <InstrumentationCard quality={data.instrumentation} meta={data.meta} />}
              </div>
            </div>
          ))}
      </ApmQueryState>
    </ApmPage>
  );
}
