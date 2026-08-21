import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { ServiceIdentity, SignalFilterHandle } from '@/api/apm';
import { cn } from '@/shell/lib/cn';
import {
  surfaceModuleNavigationActiveClass,
  surfaceModuleNavigationClass,
  surfaceModuleNavigationItemClass,
  surfaceModuleNavigationRowClass,
} from '@/shell/SurfaceWorkbench';

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
    <div
      data-apm-navigation="surface"
      className={surfaceModuleNavigationClass}
    >
      <nav
        aria-label={t('services.navigation')}
        className={surfaceModuleNavigationRowClass}
      >
        {items.map((item) => (
          <Link
            key={item.key}
            to={item.to}
            aria-current={item.key === active ? 'page' : undefined}
            className={cn(
              surfaceModuleNavigationItemClass,
              item.key === active && surfaceModuleNavigationActiveClass,
            )}
          >
            {t(`services.nav.${item.key}`)}
          </Link>
        ))}
      </nav>
    </div>
  );
}
