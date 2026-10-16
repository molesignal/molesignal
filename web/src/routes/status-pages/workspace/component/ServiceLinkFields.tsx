import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as businessServicesApi from '@/api/businessServices';
import type { ComponentServiceLink, ServiceLinkMode } from '@/api/statusPages';
import { FormField, FormRadio, FormSection, FormSelect } from '@/shell/FormDrawer';

import { componentStatusLabel } from '../../model';
import { NotifySwitch } from '../NotifySwitch';

/**
 * The business service a component stands for, and how its health reaches
 * the component: the component follows it, or the page's automation rules
 * publish incidents from it.
 */
export function ServiceLinkFields({
  link,
  onChange,
}: {
  link: ComponentServiceLink | null;
  onChange: (link: ComponentServiceLink | null) => void;
}) {
  const { t } = useTranslation('status-pages');
  const services = useQuery({
    queryKey: ['business-services'],
    queryFn: businessServicesApi.list,
    staleTime: 30_000,
  });
  const known = services.data ?? [];
  const missing = link && !known.some((service) => service.id === link.business_service_id);

  return (
    <FormSection
      title={t('forms.component.business_service')}
      description={t('forms.component.business_service_hint')}
    >
      <FormField label={t('fields.business_service')}>
        <FormSelect
          value={link?.business_service_id ?? ''}
          disabled={services.isLoading}
          onChange={(value) =>
            onChange(
              value
                ? {
                    business_service_id: value,
                    mode: link?.mode ?? 'follow',
                    notify_subscribers: link?.notify_subscribers ?? true,
                  }
                : null,
            )}
          options={[
            { value: '', label: t('forms.component.no_business_service') },
            ...(missing ? [{ value: link.business_service_id, label: t('forms.component.unknown_business_service') }] : []),
            ...known.map((service) => ({
              value: service.id,
              label: `${service.name} · ${componentStatusLabel(t, service.health.status)}`,
            })),
          ]}
        />
      </FormField>
      {!services.isLoading && known.length === 0 && (
        <p className="text-xs leading-relaxed text-tx-3">
          {t('forms.component.no_business_services')}{' '}
          <Link to="/business-services" className="text-indigo hover:underline">
            {t('forms.component.open_business_services')}
          </Link>
        </p>
      )}
      {link && (
        <>
          <FormRadio<ServiceLinkMode>
            value={link.mode}
            onChange={(mode) => onChange({ ...link, mode })}
            options={(['follow', 'automation'] as const).map((mode) => ({
              value: mode,
              label: t(`service_link_mode.${mode}`),
              hint: t(`service_link_mode.${mode}_hint`),
            }))}
          />
          {link.mode === 'follow' && (
            <NotifySwitch
              label={t('fields.notify_subscribers')}
              hint={t('forms.component.follow_notify_hint')}
              checked={link.notify_subscribers}
              onCheckedChange={(notify) => onChange({ ...link, notify_subscribers: notify })}
            />
          )}
        </>
      )}
    </FormSection>
  );
}
