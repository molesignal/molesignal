import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { ServiceIdentity, SignalFilterHandle } from '@/api/apm';
import { cn } from '@/shell/lib/cn';

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
    <nav
      aria-label={t('services.navigation')}
      className="flex min-h-11 items-stretch gap-1 overflow-x-auto border-b border-bd-0 bg-bg-1 px-6"
    >
      {items.map((item) => (
        <Link
          key={item.key}
          to={item.to}
          aria-current={item.key === active ? 'page' : undefined}
          className={cn(
            'inline-flex shrink-0 items-center border-b-2 border-transparent px-3 text-xs font-strong text-tx-2 transition-colors hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0',
            item.key === active && 'border-indigo bg-bg-2 text-tx-0',
          )}
        >
          {t(`services.nav.${item.key}`)}
        </Link>
      ))}
    </nav>
  );
}
