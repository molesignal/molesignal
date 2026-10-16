import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { BusinessService } from '@/api/businessServices';
import { Pill } from '@/shell/chrome';

import { STATUS_TONE } from '../model';
import { ServiceSection } from '../Section';

/**
 * Where customers see the service: the status-page components that stand for
 * it, and whether each follows its health or is published to by automation.
 */
export function LinkedComponents({ service }: { service: BusinessService }) {
  const { t } = useTranslation('business-services');

  return (
    <ServiceSection title={t('detail.linked_components')} description={t('detail.linked_components_hint')}>
      {service.linked_components.length === 0 ? (
        <p className="px-4 pb-4 text-xs text-tx-2">{t('detail.no_linked_components')}</p>
      ) : (
        <ul className="divide-y divide-bd-0 border-t border-bd-0">
          {service.linked_components.map((linked) => (
            <li key={linked.component_id} className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 px-4 py-2.5">
              <Link
                to={`/status-pages/${encodeURIComponent(linked.status_page_id)}/components/${encodeURIComponent(linked.component_id)}`}
                className="min-w-0 flex-1 truncate text-sm text-tx-0 hover:underline"
              >
                {linked.status_page_name} / {linked.component_name}
              </Link>
              <span className="text-xs text-tx-2">
                {t(`link_modes.${linked.mode}`)}
                {linked.mode === 'follow' && !linked.notify_subscribers && ` · ${t('detail.quiet')}`}
              </span>
              {linked.component_status !== 'maintenance' ? (
                <Pill tone={STATUS_TONE[linked.component_status]}>{t(`statuses.${linked.component_status}`)}</Pill>
              ) : (
                <Pill tone="blue">{t('statuses.maintenance')}</Pill>
              )}
            </li>
          ))}
        </ul>
      )}
    </ServiceSection>
  );
}
