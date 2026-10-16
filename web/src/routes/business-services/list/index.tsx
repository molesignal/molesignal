import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as businessServicesApi from '@/api/businessServices';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import type { ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import { ChromeButton } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';

import { ServiceEditor } from '../editor/ServiceEditor';
import { sortServices } from '../model';
import { useDependencyTargets } from '../targets';
import { BusinessServicesActivation } from './Activation';
import { ServiceRow } from './ServiceRow';

/** How often the list reads the health again; the server works it out as alerts and monitors change. */
const REFRESH_MILLIS = 30_000;

/**
 * Every business service of the organization, the ones in trouble first. A
 * business service is what customers rely on; its health comes from the
 * technical services, monitors and alert rules it depends on.
 */
export function BusinessServices() {
  const { t } = useTranslation('business-services');
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'business_services.manage' });
  const [creating, setCreating] = React.useState(false);
  const servicesQuery = useQuery({
    queryKey: ['business-services'],
    queryFn: businessServicesApi.list,
    refetchInterval: REFRESH_MILLIS,
  });
  const services = sortServices(servicesQuery.data ?? []);
  const targets = useDependencyTargets(services.length > 0);
  const nowMicros = (servicesQuery.dataUpdatedAt || Date.now()) * 1_000;

  const createMutation = useMutation({
    mutationFn: businessServicesApi.create,
    onSuccess: async (service) => {
      toast.success(t('toast.created'));
      setCreating(false);
      await queryClient.invalidateQueries({ queryKey: ['business-services'] });
      navigate(`/business-services/${encodeURIComponent(service.id)}`);
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });

  const firstRun = servicesQuery.isSuccess && services.length === 0;
  const state: ProductStateProps | null = servicesQuery.isLoading
    ? { variant: 'loading' }
    : servicesQuery.isError
      ? { variant: 'error', error: servicesQuery.error }
      : null;
  const openCreate = () => manageAccess.allowed && setCreating(true);

  return (
    <>
      <ListPage
        appearance="surface"
        title={t('title')}
        subtitle={t('subtitle')}
        // The first run carries the one way forward itself; the header does not offer it again.
        toolbar={
          firstRun ? undefined : (
            <ChromeButton
              variant="primary"
              disabled={manageAccess.disabled}
              disabledReason={manageAccess.reason}
              onClick={openCreate}
            >
              <Plus className="h-3.5 w-3.5" />
              {t('actions.create')}
            </ChromeButton>
          )
        }
        {...(firstRun ? { contentSurface: false } : { state })}
      >
        {firstRun ? (
          <BusinessServicesActivation manageAccess={manageAccess} onCreate={openCreate} />
        ) : (
          <ul aria-label={t('title')} className="divide-y divide-bd-0">
            {services.map((service) => (
              <ServiceRow key={service.id} service={service} label={targets.label} nowMicros={nowMicros} />
            ))}
          </ul>
        )}
      </ListPage>

      <ServiceEditor
        open={creating}
        service={null}
        busy={createMutation.isPending}
        onOpenChange={setCreating}
        onSubmit={(input) => createMutation.mutate(input)}
      />
    </>
  );
}
