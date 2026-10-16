import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { BusinessService } from '@/api/businessServices';
import { Pill } from '@/shell/chrome';
import { formatRelativeMicros } from '@/time/relative';

import { STATUS_TONE } from '../model';
import { Timeline } from '../Timeline';

/**
 * A business service as one row: what it is, how it is doing and since when,
 * what is behind it when it is not well, and where customers see it.
 */
export function ServiceRow({
  service,
  label,
  nowMicros,
}: {
  service: BusinessService;
  /** The name a dependency target goes by. */
  label: (kind: BusinessService['dependencies'][number]['kind'], target: string) => string;
  nowMicros: number;
}) {
  const { t, i18n } = useTranslation('business-services');
  const { health } = service;
  const causes = health.causes.slice(0, 3).map((cause) => label(cause.kind, cause.target));

  return (
    <li>
      <Link
        to={`/business-services/${encodeURIComponent(service.id)}`}
        className="grid min-w-0 grid-cols-1 gap-x-6 gap-y-2 px-4 py-3 transition-colors hover:bg-bg-2 focus-visible:bg-bg-2 lg:grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)_200px]"
      >
        <div className="min-w-0">
          <div className="flex min-w-0 items-center gap-2">
            <span className="truncate text-sm font-strong text-tx-0">{service.name}</span>
            <Pill tone={STATUS_TONE[health.status]}>{t(`statuses.${health.status}`)}</Pill>
          </div>
          <p className="mt-0.5 truncate text-xs text-tx-2">
            {health.status === 'operational'
              ? service.description || t('list.dependency_count', { count: service.dependencies.length })
              : t('list.causes', {
                  names: causes.join(t('list.name_separator')),
                  count: health.causes.length,
                })}
          </p>
        </div>
        <div className="min-w-0 text-xs text-tx-2">
          <div>
            {health.changed_at
              ? t('list.since', { time: formatRelativeMicros(health.changed_at, i18n.language, nowMicros) })
              : t('list.not_evaluated')}
          </div>
          <div className="mt-0.5 truncate">
            {service.linked_components.length > 0
              ? t('list.linked_components', { count: service.linked_components.length })
              : t('list.no_linked_components')}
          </div>
        </div>
        <Timeline history={health.history} nowMicros={nowMicros} compact />
      </Link>
    </li>
  );
}
