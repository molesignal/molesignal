import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable, PageHeader } from '@/admin';
import * as domainsApi from '@/api/domains';
import { toApiError } from '@/lib/http';
import {
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { ProductState, productStateFor, useLicenseErrorGate } from '@/product/states';
import { ChromeButton, Pill } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { SectionBody } from './_atoms';
import { formatMicros } from '../rum/_helpers';

export function DomainManagement() {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const manageAccess = useActionAccess({
    permission: 'org.settings.manage',
    feature: 'domain_management',
  });
  const [creating, setCreating] = React.useState(false);
  const [removing, setRemoving] = React.useState<domainsApi.Domain | null>(null);
  const licenseGate = useLicenseErrorGate();

  const q = useQuery({
    queryKey: ['domains'],
    queryFn: () => domainsApi.list(),
  });
  const rows = q.data ?? [];
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: rows });
  const pageState =
    (state === 'error' ? licenseGate(q.error, 'features.domain_management') : null) ??
    productStateFor(state, {
      error: q.error,
      emptyTitle: t('domain_management.empty_title'),
      emptyDescription: t('domain_management.empty_description'),
    });

  const renew = useMutation({
    mutationFn: (id: string) => domainsApi.renew(id),
    onSuccess: () => {
      toast.success(t('domain_management.toast_renew_queued'));
      void qc.invalidateQueries({ queryKey: ['domains'] });
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const remove = useMutation({
    mutationFn: (id: string) => domainsApi.remove(id),
    onSuccess: () => {
      toast.success(tc('status.deleted'));
      void qc.invalidateQueries({ queryKey: ['domains'] });
      setRemoving(null);
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  return (
    <>
      <PageHeader
        title={t('domain_management.title')}
        subtitle={t('domain_management.subtitle') as string}
        actions={
          <ChromeButton
            variant="primary"
            onClick={() => setCreating(true)}
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
          >
            {t('domain_management.new_domain')}
          </ChromeButton>
        }
      />
      <CreateDrawer
        open={creating}
        access={manageAccess}
        onClose={() => setCreating(false)}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(v) => !v && setRemoving(null)}
        destructive
        title={t('domain_management.delete_confirm_title')}
        description={t('domain_management.delete_confirm_description')}
        confirmLabel={tc('actions.delete')}
        busy={remove.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (removing && manageAccess.allowed) {
            remove.mutate(removing.id);
          }
        }}
      />
      <SectionBody>
        {pageState ? (
          <ProductState {...pageState} />
        ) : (
          <DataTable
            rows={rows}
            rowKey={(r) => r.id}
            columns={[
              {
                key: 'hostname',
                header: t('domain_management.columns.hostname'),
                cell: (r) => <span className="font-sans text-tx-0">{r.hostname}</span>,
              },
              {
                key: 'state',
                header: t('domain_management.columns.state'),
                cell: (r) => (
                  <Pill tone={r.state === 'active' ? 'green' : r.state === 'failed' ? 'red' : 'yellow'}>
                    {r.state}
                  </Pill>
                ),
                width: 120,
              },
              {
                key: 'expires',
                header: t('domain_management.columns.cert_expires'),
                cell: (r) => formatMicros(r.cert_not_after_micros),
                width: 200,
              },
              {
                key: 'last_error',
                header: t('domain_management.columns.last_error'),
                cell: (r) => r.last_error ?? '—',
              },
              {
                key: 'actions',
                header: '',
                width: 160,
                cell: (r) => (
                  <div
                    className="flex justify-end gap-1"
                    onClick={(event) => event.stopPropagation()}
                  >
                    <ChromeButton
                      variant="ghost"
                      size="sm"
                      disabled={manageAccess.disabled || renew.isPending}
                      disabledReason={
                        manageAccess.reason ??
                        (renew.isPending
                          ? tc('access.operation_pending')
                          : undefined)
                      }
                      onClick={(e) => {
                        e.stopPropagation();
                        renew.mutate(r.id);
                      }}
                    >
                      {t('domain_management.renew')}
                    </ChromeButton>
                    <ChromeButton
                      variant="ghost"
                      size="sm"
                      disabled={manageAccess.disabled || remove.isPending}
                      disabledReason={
                        manageAccess.reason ??
                        (remove.isPending
                          ? tc('access.operation_pending')
                          : undefined)
                      }
                      onClick={(e) => {
                        e.stopPropagation();
                        setRemoving(r);
                      }}
                      className="enabled:hover:text-red-soft"
                    >
                      {tc('actions.delete')}
                    </ChromeButton>
                  </div>
                ),
              },
            ]}
          />
        )}
      </SectionBody>
    </>
  );
}

function CreateDrawer({
  open,
  access,
  onClose,
}: {
  open: boolean;
  access: ActionAccess;
  onClose: () => void;
}) {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const [hostname, setHostname] = React.useState('');

  React.useEffect(() => {
    if (!open) setHostname('');
  }, [open]);

  const create = useMutation({
    mutationFn: () => domainsApi.create({ hostname }),
    onSuccess: () => {
      toast.success(t('domain_management.toast_added'));
      void qc.invalidateQueries({ queryKey: ['domains'] });
      onClose();
    },
    onError: (err) => toast.error(toApiError(err).message),
  });
  const invalid = hostname.trim().length === 0;

  return (
    <FormDrawer
      open={open}
      onOpenChange={(v) => !v && onClose()}
      title={t('domain_management.drawer_title')}
      footer={
        <FormSubmitFooter
          busy={create.isPending}
          disabled={access.disabled}
          invalid={invalid}
          disabledReason={
            access.reason ??
            (invalid ? tc('access.form_invalid') : undefined)
          }
          onCancel={onClose}
          submitLabel={t('domain_management.submit_label')}
          formId="domain-form"
        />
      }
    >
      <form
        id="domain-form"
        onSubmit={(e) => {
          e.preventDefault();
          if (access.disabled || invalid || create.isPending) return;
          create.mutate();
        }}
      >
        <FormSection>
          <FormField label={t('domain_management.field_hostname')} required>
            <FormInput
              value={hostname}
              onChange={(e) => setHostname(e.target.value)}
              placeholder="logs.example.com"
              disabled={access.disabled || create.isPending}
              disabledReason={
                access.reason ??
                (create.isPending
                  ? tc('access.operation_pending')
                  : undefined)
              }
              required
            />
          </FormField>
        </FormSection>
      </form>
    </FormDrawer>
  );
}
