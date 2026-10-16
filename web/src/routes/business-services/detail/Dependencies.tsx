import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { BusinessService, BusinessServiceDependencyKind } from '@/api/businessServices';
import { Pill } from '@/shell/chrome';

import { dependencyHref } from '../model';
import { ServiceSection } from '../Section';

/** What the service depends on, and how much of each one's trouble reaches it. */
export function ServiceDependencies({
  service,
  label,
}: {
  service: BusinessService;
  label: (kind: BusinessServiceDependencyKind, target: string) => string;
}) {
  const { t } = useTranslation('business-services');

  return (
    <ServiceSection
      title={t('detail.dependencies')}
      description={t('detail.dependencies_hint')}
    >
      {service.dependencies.length === 0 ? (
        <p className="px-4 pb-4 text-xs text-tx-2">{t('detail.no_dependencies')}</p>
      ) : (
        <ul className="divide-y divide-bd-0 border-t border-bd-0">
          {service.dependencies.map((dependency) => (
            <li
              key={`${dependency.kind}:${dependency.target}`}
              className="flex min-w-0 items-center gap-3 px-4 py-2.5"
            >
              <span className="w-24 shrink-0 text-xs text-tx-3">{t(`dependency_kinds.${dependency.kind}`)}</span>
              <Link
                to={dependencyHref(dependency.kind, dependency.target)}
                className="min-w-0 flex-1 truncate text-sm text-tx-0 hover:underline"
              >
                {label(dependency.kind, dependency.target)}
              </Link>
              <Pill tone={dependency.impact === 'outage' ? 'neutral' : 'dim'}>
                {t(`dependency_impacts.${dependency.impact}`)}
              </Pill>
            </li>
          ))}
        </ul>
      )}
    </ServiceSection>
  );
}
