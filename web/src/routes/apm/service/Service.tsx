import { useQuery } from '@tanstack/react-query';
import { Star } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Navigate } from 'react-router-dom';

import { apmApi, apmQueryKeys } from '@/api/apm';
import { ModuleTabs } from '@/shell/tabs/ModuleTabs';

import { SERVICE_SECTIONS, serviceKey, servicePath } from '../scope';
import { ServiceDependencies } from './Dependencies';
import { ServiceDeployments } from './Deployments';
import { ServiceDrawers } from './Drawers';
import { ServiceEndpoints } from './Endpoints';
import { ServiceErrors } from './Errors';
import { ServiceFacts } from './Facts';
import { ServiceScopeRow } from './Header';
import { ServiceOverview } from './Overview';
import { ServiceRuntime } from './Runtime';
import { ServiceSettings } from './Settings';
import { ServiceTraces } from './Traces';
import { type ServiceAddress, useServiceAddress, useServiceDetail, useServiceEnvironments } from './useService';
import { FilterToggle } from '../frame/FilterToggle';
import { ApmScopeRow } from '../frame/Navigation';
import { ApmPage, ApmQueryState } from '../frame/Page';
import { ApmTimeControls } from '../frame/TimeControls';
import { TraceDrawerHost } from '../frame/TraceDrawerHost';
import { useApmFavorites } from '../frame/useApmFavorites';

/** Sections whose charts a comparison can go under. */
const COMPARABLE = new Set(['overview', 'endpoints', 'dependencies']);

/**
 * A service's own page: the service in one environment, said in a line after
 * its name (health, system, type, stack, versions, team, instances, last
 * deploy, region, repository), its own navigation card (its sections, the
 * time controls, its environment and version, documents and alerts), and the
 * section open. Endpoints and error groups open in drawers over any section,
 * traces in the trace drawer.
 */
export function ApmService() {
  const { t } = useTranslation('apm');
  const address = useServiceAddress();
  const { detail, compared } = useServiceDetail(address);
  const environments = useServiceEnvironments(address);
  const definitions = useQuery({
    queryKey: apmQueryKeys.definitions(address.orgId),
    queryFn: apmApi.definitions,
    enabled: Boolean(address.orgId),
  });
  const definition = definitions.data?.find((entry) => serviceKey(entry) === serviceKey(address.identity)) ?? null;

  if (!address.section) return <Navigate to={servicePath(address.identity, 'overview', address.searchParams)} replace />;

  const section = address.section;
  const data = detail.data;
  const versions = (data?.versions ?? []).map((version) => version.version);

  return (
    <TraceDrawerHost>
      <ApmPage
        page="service"
        title={address.name}
        toolbar={<FavoriteToggle address={address} />}
        subtitle={<ServiceFacts address={address} summary={detail.data?.service} versions={detail.data?.versions ?? []} definition={definition} />}
        breadcrumbs={[
          { labelKey: 'apm', to: '/apm/services' },
          { labelKey: 'breadcrumbs.apm_services', to: '/apm/services' },
          { labelKey: 'breadcrumbs.apm_service_detail', label: address.name },
        ]}
        navigation={
          <ModuleTabs
            label={t('service.navigation')}
            dataAttributes={{ 'data-apm-navigation': 'service' }}
            end={
              <ApmTimeControls
                scope={address.scope}
                onScopeChange={address.setScope}
                comparable={COMPARABLE.has(section)}
                environments={environments.filter((environment) => environment !== address.environment)}
              />
            }
            items={SERVICE_SECTIONS.map((key) => ({
              key,
              to: servicePath(address.identity, key, address.searchParams),
              active: key === section,
              label: t(`service.sections.${key}`),
            }))}
          >
            <ApmScopeRow>
              <ServiceScopeRow address={address} summary={data?.service} versions={versions} meta={data?.meta} />
            </ApmScopeRow>
          </ModuleTabs>
        }
      >
        <ApmQueryState query={detail} empty={false}>
          {data && (
            <>
              {section === 'overview' && <ServiceOverview address={address} detail={data} compared={compared?.data} />}
              {section === 'endpoints' && <ServiceEndpoints address={address} detail={data} />}
              {section === 'traces' && <ServiceTraces address={address} />}
              {section === 'dependencies' && <ServiceDependencies address={address} detail={data} compared={compared?.data} />}
              {section === 'errors' && <ServiceErrors address={address} />}
              {section === 'deployments' && <ServiceDeployments address={address} detail={data} />}
              {section === 'runtime' && <ServiceRuntime address={address} summary={data.service} />}
              {section === 'settings' && <ServiceSettings address={address} summary={data.service} definition={definition} transactions={data.transactions} />}
            </>
          )}
        </ApmQueryState>
        <ServiceDrawers address={address} />
      </ApmPage>
    </TraceDrawerHost>
  );
}

/** Keeps the service among the reader's favorites, at the top of the catalog, or takes it out. */
function FavoriteToggle({ address }: { address: ServiceAddress }) {
  const { t } = useTranslation('apm');
  const favorites = useApmFavorites();
  const on = favorites.isFavorite(address);

  return (
    <FilterToggle
      pressed={on}
      spring
      onToggle={() => favorites.toggle(address)}
      icon={<Star aria-hidden className="h-3.5 w-3.5 text-[var(--yellow-ink)]" fill={on ? 'currentColor' : 'none'} />}
      data-apm-service-favorite={on ? 'on' : 'off'}
    >
      {t(on ? 'service.favorited' : 'service.favorite')}
    </FilterToggle>
  );
}
