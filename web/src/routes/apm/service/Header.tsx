import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { type ApmMeta, apmApi, apmQueryKeys, type ServiceSummary } from '@/api/apm';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { AlertCount, incidentsAbout, NewApmAlert, useActiveIncidents } from '../frame/alerts';
import { InvestigateButton } from '../frame/investigate';
import { ApmQualityChip } from '../frame/QualityChip';
import { serviceKey, servicePath, writeApmScope } from '../scope';
import { ServiceDocs } from './Docs';
import { RelatedSignals } from './RelatedSignals';
import { type ServiceAddress, useServiceEnvironments } from './useService';

const ALL = '__all__';
const SELECT = 'h-7 border-0 bg-[var(--control-surface)] px-2 text-xs font-strong text-tx-1 hover:bg-bg-3';

/**
 * The second row of a service's navigation card: which of its environments
 * and versions the page shows, its documents and alerts, and the state of
 * the data, with its other signals at the end, lit where they carry the
 * service. Who owns the service and how it stands are said after its name;
 * its catalog entry is kept in its Settings.
 */
export function ServiceScopeRow({
  address,
  summary,
  versions,
  meta,
}: {
  address: ServiceAddress;
  summary: ServiceSummary | undefined;
  versions: readonly string[];
  meta: ApmMeta | undefined;
}) {
  const { t } = useTranslation('apm');
  const navigate = useNavigate();
  const environments = useServiceEnvironments(address);
  const definitions = useQuery({
    queryKey: apmQueryKeys.definitions(address.orgId),
    queryFn: apmApi.definitions,
    enabled: Boolean(address.orgId),
  });
  const definition = definitions.data?.find((entry) => serviceKey(entry) === serviceKey(address.identity)) ?? null;
  const incidents = useActiveIncidents();
  const alerts = incidentsAbout(incidents.data, { name: address.name, environment: address.environment });

  return (
    <>
      <FittedSelect
        data-apm-environment=""
        aria-label={t('service.environment')}
        value={address.environment}
        onValueChange={(environment) =>
          navigate(servicePath({ ...address.identity, environment }, address.section ?? 'overview', withoutVersion(address.searchParams)))
        }
        options={(environments.length ? environments : [address.environment]).map((value) => ({ value, label: value }))}
        maxWidth={200}
        prefix={t('service.environment')}
        className={SELECT}
      />
      <FittedSelect
        data-apm-version=""
        aria-label={t('service.version')}
        value={address.scope.version || ALL}
        onValueChange={(version) => address.setScope({ version: version === ALL ? '' : version })}
        options={[{ value: ALL, label: t('service.all_versions') }, ...versions.map((value) => ({ value, label: value }))]}
        maxWidth={200}
        prefix={address.scope.version ? t('service.version') : undefined}
        className={SELECT}
      />
      <span aria-hidden className="h-4 w-px bg-bd-0" />
      <ServiceDocs name={address.name} teamId={definition?.teamId} />
      <NewApmAlert service={address.identity} />
      <InvestigateButton subject={{ service: address.identity, range: address.range }} />
      <AlertCount incidents={alerts} />
      <ApmQualityChip meta={meta} />
      {summary && <RelatedSignals address={address} summary={summary} />}
    </>
  );
}

/** The address of the page with the version reset: switching the environment starts from every version. */
export function withoutVersion(params: URLSearchParams): URLSearchParams {
  return writeApmScope(params, { version: '' });
}
