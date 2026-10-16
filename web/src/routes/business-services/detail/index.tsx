import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Pencil, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';

import { ConfirmDialog } from '@/admin';
import * as businessServicesApi from '@/api/businessServices';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import type { ProductStateProps } from '@/product/states';
import { DetailPage } from '@/product/templates';
import { ChromeButton } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';

import { ServiceEditor } from '../editor/ServiceEditor';
import { useDependencyTargets } from '../targets';
import { ServiceDependencies } from './Dependencies';
import { ServiceHealth } from './Health';
import { LinkedComponents } from './LinkedComponents';

const REFRESH_MILLIS = 30_000;

/**
 * One business service: how it is doing and why, what it depends on, and
 * where customers see it. The same health drives the components that stand
 * for it on status pages.
 */
export function BusinessServiceDetail() {
  const { t } = useTranslation('business-services');
  const { serviceId = '' } = useParams();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'business_services.manage' });
  const [editing, setEditing] = React.useState(false);
  const [deleting, setDeleting] = React.useState(false);
  const serviceQuery = useQuery({
    queryKey: ['business-services', serviceId],
    queryFn: () => businessServicesApi.get(serviceId),
    enabled: Boolean(serviceId),
    refetchInterval: REFRESH_MILLIS,
  });
  const service = serviceQuery.data;
  const targets = useDependencyTargets(Boolean(service), serviceId);
  const nowMicros = (serviceQuery.dataUpdatedAt || Date.now()) * 1_000;

  const refresh = () => queryClient.invalidateQueries({ queryKey: ['business-services'] });
  const updateMutation = useMutation({
    mutationFn: (input: businessServicesApi.BusinessServiceInput) =>
      businessServicesApi.update(serviceId, input),
    onSuccess: async () => {
      toast.success(t('toast.saved'));
      setEditing(false);
      await refresh();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });
  const deleteMutation = useMutation({
    mutationFn: () => businessServicesApi.remove(serviceId),
    onSuccess: async () => {
      toast.success(t('toast.deleted'));
      setDeleting(false);
      await refresh();
      navigate('/business-services', { replace: true });
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });

  const state: ProductStateProps | null = serviceQuery.isLoading
    ? { variant: 'loading' }
    : serviceQuery.isError
      ? { variant: 'error', error: serviceQuery.error }
      : null;

  return (
    <>
      <DetailPage
        appearance="surface"
        title={service?.name ?? t('title')}
        subtitle={service?.description || undefined}
        state={state}
        toolbar={
          service && (
            <>
              <ChromeButton
                disabled={manageAccess.disabled}
                disabledReason={manageAccess.reason}
                onClick={() => setEditing(true)}
              >
                <Pencil className="h-3.5 w-3.5" />
                {t('actions.edit')}
              </ChromeButton>
              <ChromeButton
                disabled={manageAccess.disabled}
                disabledReason={manageAccess.reason}
                className="enabled:hover:text-red-soft"
                onClick={() => setDeleting(true)}
              >
                <Trash2 className="h-3.5 w-3.5" />
                {t('actions.delete')}
              </ChromeButton>
            </>
          )
        }
      >
        {service && (
          <div className="grid grid-cols-1 items-start gap-[12px] xl:grid-cols-[minmax(0,1.35fr)_minmax(0,1fr)]">
            <ServiceHealth service={service} label={targets.label} nowMicros={nowMicros} />
            <div className="space-y-[12px]">
              <ServiceDependencies service={service} label={targets.label} />
              <LinkedComponents service={service} />
            </div>
          </div>
        )}
      </DetailPage>

      <ServiceEditor
        open={editing && Boolean(service)}
        service={service ?? null}
        busy={updateMutation.isPending}
        onOpenChange={setEditing}
        onSubmit={(input) => updateMutation.mutate(input)}
      />
      <ConfirmDialog
        open={deleting}
        onOpenChange={setDeleting}
        title={t('confirm.delete_title')}
        description={t('confirm.delete_description', {
          name: service?.name,
          count: service?.linked_components.length ?? 0,
        })}
        confirmLabel={t('actions.delete')}
        destructive
        busy={deleteMutation.isPending}
        onConfirm={() => deleteMutation.mutate()}
      />
    </>
  );
}
