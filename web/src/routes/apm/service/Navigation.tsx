import { useTranslation } from 'react-i18next';

import type { ServiceIdentity, SignalFilterHandle } from '@/api/apm';
import { ModuleTabs } from '@/shell/tabs/ModuleTabs';

import { signalHref } from '../model';

type ServiceSection =
  | 'overview'
  | 'transactions'
  | 'traces'
  | 'dependencies'
  | 'errors'
  | 'runtime'
  | 'deployments';

export function ServiceNavigation({
  active,
  service,
  traces,
  version,
}: {
  active: ServiceSection;
  service: ServiceIdentity;
  traces: SignalFilterHandle;
  version?: string;
}) {
  const { t } = useTranslation('apm');
  const query = new URLSearchParams({
    namespace: service.namespace,
    service: service.name,
    environment: service.environment,
  });
  if (version) query.set('version', version);
  const suffix = `?${query}`;
  const servicePath = `/apm/services/${encodeURIComponent(service.name)}`;
  const items: Array<{ key: ServiceSection; to: string }> = [
    { key: 'overview', to: `${servicePath}${suffix}` },
    { key: 'transactions', to: `/apm/transactions${suffix}` },
    { key: 'traces', to: signalHref('traces', traces) },
    { key: 'dependencies', to: `/apm/dependencies${suffix}` },
    { key: 'errors', to: `/apm/errors${suffix}` },
    { key: 'runtime', to: `${servicePath}/runtime${suffix}` },
    { key: 'deployments', to: `/apm/deployments${suffix}` },
  ];

  return (
    <ModuleTabs
      label={t('services.navigation')}
      dataAttributes={{ 'data-apm-navigation': 'surface' }}
      items={items.map((item) => ({
        key: item.key,
        to: item.to,
        active: item.key === active,
        label: t(`services.nav.${item.key}`),
      }))}
    />
  );
}
