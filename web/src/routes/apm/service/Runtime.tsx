import { useQuery } from '@tanstack/react-query';
import { Cpu, RadioTower, ServerCog } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useParams } from 'react-router-dom';

import { apmApi, apmQueryKeys } from '@/api/apm';

import { ApmPageFrame, QueryBoundary, Section } from '../components';
import { ApmFilters } from '../Filters';
import { formatCount } from '../format';
import { useApmFilters } from '../useApmFilters';
import { ServiceNavigation } from './Navigation';

export function ApmServiceRuntime() {
  const { t } = useTranslation('apm');
  const { service = '' } = useParams();
  const { orgId, filters, params, setFilter, clearFilters } = useApmFilters();
  const query = useQuery({
    queryKey: apmQueryKeys.service(orgId, service, params),
    queryFn: () => apmApi.service(service, params),
    enabled: Boolean(orgId && service),
    staleTime: 30_000,
  });
  const detail = query.data;

  return (
    <ApmPageFrame
      title={detail?.service.service.name ?? service}
      subtitle={t('runtime.subtitle')}
      meta={detail?.meta}
      navigation={
        detail ? (
          <ServiceNavigation
            active="runtime"
            service={detail.service.service}
            traces={detail.service.traces}
            version={filters.version}
          />
        ) : null
      }
      toolbar={
        <ApmFilters
          filters={filters}
          setFilter={setFilter}
          clearFilters={clearFilters}
          showSearch={false}
          showService={false}
        />
      }
    >
      <QueryBoundary
        pending={query.isPending}
        error={query.error}
        empty={false}
        refetching={query.isFetching && Boolean(detail)}
        onRetry={() => void query.refetch()}
      >
        {detail && (
          <div className="space-y-5">
            <Section
              title={t('runtime.title')}
              description={t('runtime.description')}
            >
              <div className="grid gap-px bg-bd-0 md:grid-cols-3">
                <RuntimeFact
                  icon={Cpu}
                  label={t('runtime.language')}
                  value={detail.service.instrumentation.runtime_language ?? t('values.unknown')}
                />
                <RuntimeFact
                  icon={RadioTower}
                  label={t('runtime.sdk')}
                  value={detail.service.instrumentation.telemetry_sdk_name ?? t('values.unknown')}
                  detail={detail.service.instrumentation.telemetry_sdk_version}
                />
                <RuntimeFact
                  icon={ServerCog}
                  label={t('runtime.instances')}
                  value={formatCount(
                    detail.service.instrumentation.recent_instance_count,
                  )}
                />
              </div>
            </Section>
          </div>
        )}
      </QueryBoundary>
    </ApmPageFrame>
  );
}

function RuntimeFact({
  icon: Icon,
  label,
  value,
  detail,
}: {
  icon: typeof Cpu;
  label: string;
  value: string;
  detail?: string | undefined;
}) {
  return (
    <div className="bg-bg-1 p-5">
      <Icon aria-hidden className="h-4 w-4 text-indigo-soft" />
      <div className="mt-4 text-xs font-strong text-tx-3">{label}</div>
      <div className="mt-1 text-base font-display-strong text-tx-0">{value}</div>
      {detail && <div className="mt-1 font-mono text-xs text-tx-2">{detail}</div>}
    </div>
  );
}
