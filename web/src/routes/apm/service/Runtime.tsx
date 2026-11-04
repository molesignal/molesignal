import { useQueries, useQuery } from '@tanstack/react-query';
import { Cpu } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link, useSearchParams } from 'react-router-dom';

import { apmApi, apmQueryKeys, type ServiceSummary } from '@/api/apm';
import { fetchMetricCatalog } from '@/api/metricsCatalog';
import { runQuery } from '@/api/query';
import { promqlStepMicros } from '@/lib/promqlRange';
import { FittedSelect } from '@/shell/ui/fitted-select';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import { instanceLabel, ServiceInstances } from './Instances';
import { RuntimeProfile } from './RuntimeProfile';
import type { ServiceAddress } from './useService';
import { ApmCard, ApmQueryState } from '../frame/Page';
import { chartsOf, familiesFor, RUNTIME_FAMILIES } from '../runtime/families';
import { runtimeStatement, seriesOfResult } from '../runtime/metrics';
import { OTHER_DIMENSION } from '../scope';

/** Steps a runtime card is drawn with: plenty for a card a third of the page wide. */
const RUNTIME_CHART_STEPS = 300;

const ALL = '__all__';

/**
 * How the service's processes run, as ARMS's instance monitoring shows it:
 * the instances that ran it and what each served; the runtime metrics it
 * sends, the process's first, then those of the runtime of its language
 * (JVM, Go, Node.js, .NET, CPython, Ruby, PHP, Tokio, BEAM), a line for each
 * instance or the lines of one; and where its time goes, from its profiles.
 */
export function ServiceRuntime({ address, summary }: { address: ServiceAddress; summary: ServiceSummary | undefined }) {
  const { t } = useTranslation('apm');
  const [searchParams, setSearchParams] = useSearchParams();
  const params = address.queryParams(address.range);
  const instances = useQuery({
    queryKey: apmQueryKeys.instances(address.orgId, address.name, params),
    queryFn: () => apmApi.instances(address.name, params),
    enabled: Boolean(address.orgId && address.name),
    refetchInterval: 60_000,
  });
  const chosen = searchParams.get('instance') ?? '';
  const chosenInstance = instances.data?.find((instance) => instance.instanceId === chosen);
  const filter = chosenInstance ? instanceLabel(chosenInstance) : null;
  const choose = (instanceId: string) =>
    setSearchParams(
      (current) => {
        const next = new URLSearchParams(current);
        if (instanceId) next.set('instance', instanceId);
        else next.delete('instance');
        return next;
      },
      { replace: true },
    );

  const languages = [...(summary?.profile?.languages ?? []).map((entry) => entry.value), summary?.instrumentation.runtimeLanguage].filter(
    (language): language is string => Boolean(language),
  );
  const searched = languages.length > 0 ? familiesFor(languages, new Set()) : RUNTIME_FAMILIES;
  const prefixes = [...new Set(searched.flatMap((family) => family.prefixes))];
  const catalogs = useQueries({
    queries: prefixes.map((prefix) => ({
      queryKey: ['metrics', 'catalog', 'runtime', prefix],
      queryFn: () => fetchMetricCatalog({ q: prefix, limit: 200 }),
      staleTime: 5 * 60_000,
    })),
  });
  const pending = catalogs.some((query) => query.isPending);
  const names = new Set(catalogs.flatMap((query) => (query.data?.items ?? []).filter((item) => item.labels.includes('service.name')).map((item) => item.name)));
  const families = familiesFor(languages, names)
    .map((family) => ({ family, charts: chartsOf(family, names) }))
    .filter((entry) => entry.charts.length > 0);
  const drawn = families.flatMap(({ family, charts }) => charts.map((chart) => ({ family: family.key, ...chart })));
  const start = address.range.from.getTime() * 1_000;
  const end = address.range.to.getTime() * 1_000;

  const answers = useQueries({
    queries: drawn.map(({ family, spec, candidate }) => {
      const statement = runtimeStatement(candidate, { name: address.name, namespace: address.namespace }, filter);
      return {
        queryKey: ['apm', 'runtime', address.orgId, family, spec.chart, statement, start, end],
        queryFn: () => runQuery({ orgId: address.orgId, language: 'promql', statement, timeRange: { start, end }, step: promqlStepMicros(start, end, null, RUNTIME_CHART_STEPS), limit: 5_000 }),
        enabled: Boolean(address.orgId),
      };
    }),
  });
  const answerOf = (family: string, chart: string) => answers[drawn.findIndex((entry) => entry.family === family && entry.spec.chart === chart)];
  const choices = (instances.data ?? []).filter((instance) => instance.instanceId !== OTHER_DIMENSION);

  return (
    <div className="space-y-[12px]">
      <ServiceInstances address={address} query={instances} chosen={chosen} onChoose={choose} />
      {pending ? (
        <ApmQueryState query={{ isPending: true, isError: false, error: null, refetch: () => undefined }} empty={false}>
          {null}
        </ApmQueryState>
      ) : families.length === 0 ? (
        <ApmCard testId="runtime-empty">
          <div className="space-y-2 px-4 py-6 font-sans text-xs text-tx-2">
            <p className="font-strong text-tx-0">{t('runtime.none_title')}</p>
            <p className="leading-relaxed">{t('runtime.none_description')}</p>
            <Link to="/datasource/applications/opentelemetry?signal=metrics" className="inline-flex font-strong text-indigo-soft hover:underline">
              {t('runtime.none_action')}
            </Link>
          </div>
        </ApmCard>
      ) : (
        <>
          <div className="flex items-center gap-2 px-1 font-sans text-xs text-tx-2">
            <FittedSelect
              aria-label={t('runtime.instance')}
              data-apm-runtime-instance=""
              value={chosenInstance ? chosen : ALL}
              onValueChange={(value) => choose(value === ALL ? '' : value)}
              options={[{ value: ALL, label: t('runtime.all_instances') }, ...choices.map((instance) => ({ value: instance.instanceId, label: instance.instanceId }))]}
              maxWidth={280}
              prefix={t('runtime.instance')}
              className="h-8 border-0 bg-[var(--control-surface)] px-2.5 text-xs font-strong text-tx-1 hover:bg-bg-3"
            />
            <span className="text-tx-3">{t(chosenInstance ? 'runtime.one_instance' : 'runtime.each_instance')}</span>
          </div>
          {families.map(({ family, charts }) => (
            <section key={family.key} data-apm-runtime-family={family.key} aria-label={t(`runtime.families.${family.key}`)} className="space-y-2">
              <h3 className="flex items-center gap-1.5 px-1 font-sans text-sm font-strong text-tx-1">
                <Cpu aria-hidden className="h-3.5 w-3.5 text-[var(--chart-7)]" />
                {t(`runtime.families.${family.key}`)}
              </h3>
              <div className="grid min-w-0 gap-[12px] lg:grid-cols-2 2xl:grid-cols-3">
                {charts.map(({ spec, candidate }) => {
                  const query = answerOf(family.key, spec.chart)!;
                  const series = query.data ? seriesOfResult(query.data, spec.unit) : [];
                  const title = t(`runtime.charts.${spec.chart}`);

                  return (
                    <ApmCard key={spec.chart} title={title} description={candidate.metric} testId={`runtime-${family.key}-${spec.chart}`} bodyClassName="px-2 pb-2 pt-1">
                      <ApmQueryState query={query} empty={series.length === 0} emptyTitle={t('runtime.chart_empty')} compact>
                        <TimeSeriesChart
                          series={series}
                          xDomain={[start, end]}
                          height={180}
                          ariaLabel={title}
                          showLegend={series.length > 1}
                          legendDensity="compact"
                          options={{
                            drawStyle: 'line',
                            showPoints: 'never',
                            tooltipMode: 'all',
                            compactAxes: true,
                            leftAxis: { scale: 'linear', min: 0, ...(spec.unit ? { unit: spec.unit } : {}) },
                          }}
                        />
                      </ApmQueryState>
                    </ApmCard>
                  );
                })}
              </div>
            </section>
          ))}
        </>
      )}
      <RuntimeProfile address={address} />
    </div>
  );
}
