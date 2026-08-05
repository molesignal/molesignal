import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable, PageHeader } from '@/admin';
import * as modelPricingApi from '@/api/modelPricing';
import { toApiError } from '@/lib/http';
import {
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { ProductState, productStateFor } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
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

export function ModelPricing() {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const manageAccess = useActionAccess({
    permission: 'sys.settings.manage',
  });
  const [creating, setCreating] = React.useState(false);
  const [editing, setEditing] =
    React.useState<modelPricingApi.ModelPrice | null>(null);
  const [removing, setRemoving] = React.useState<modelPricingApi.ModelPrice | null>(null);

  const q = useQuery({
    queryKey: ['model-prices'],
    queryFn: () => modelPricingApi.list(),
    retry: false,
  });
  const rows = q.data ?? [];
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: rows });
  const pageState = productStateFor(state, {
    error: q.error,
    emptyTitle: t('model_pricing.empty_title'),
    emptyDescription: t('model_pricing.empty_description'),
  });

  const remove = useMutation({
    mutationFn: (row: modelPricingApi.ModelPrice) => modelPricingApi.remove(row.provider, row.model),
    onSuccess: () => {
      toast.success(tc('status.deleted'));
      void qc.invalidateQueries({ queryKey: ['model-prices'] });
      setRemoving(null);
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  return (
    <>
      <PageHeader
        title={t('model_pricing.title')}
        subtitle={t('model_pricing.subtitle') as string}
        actions={
          <ChromeButton
            variant="primary"
            onClick={() => setCreating(true)}
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
          >
            {t('model_pricing.new_price')}
          </ChromeButton>
        }
      />
      <UpsertDrawer
        open={creating || editing !== null}
        initial={editing}
        access={manageAccess}
        onClose={() => {
          setCreating(false);
          setEditing(null);
        }}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(v) => !v && setRemoving(null)}
        destructive
        title={t('model_pricing.delete_title')}
        description={removing ? `${removing.provider}/${removing.model}` : ''}
        confirmLabel={tc('actions.delete')}
        busy={remove.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (removing && manageAccess.allowed) {
            remove.mutate(removing);
          }
        }}
      />
      <SectionBody>
        {pageState ? (
          <ProductState {...pageState} />
        ) : (
          <DataTable
            rows={rows}
            rowKey={(r) => `${r.provider}/${r.model}`}
            onRowClick={(row) => setEditing(row)}
            isRowClickDisabled={() => manageAccess.disabled}
            rowClickDisabledReason={() => manageAccess.reason}
            columns={[
              {
                key: 'provider',
                header: t('model_pricing.columns.provider'),
                cell: (r) => r.provider,
                width: 140,
              },
              { key: 'model', header: t('model_pricing.columns.model'), cell: (r) => r.model },
              {
                key: 'prompt',
                header: t('model_pricing.columns.prompt'),
                cell: (r) => r.prompt_usd_per_1k.toFixed(6),
                width: 140,
              },
              {
                key: 'completion',
                header: t('model_pricing.columns.completion'),
                cell: (r) => r.completion_usd_per_1k.toFixed(6),
                width: 160,
              },
              {
                key: 'updated',
                header: t('model_pricing.columns.updated'),
                cell: (r) => formatMicros(r.updated_at_micros),
                width: 200,
              },
              {
                key: 'actions',
                header: '',
                width: 170,
                cell: (r) => (
                  <div
                    className="flex justify-end gap-1"
                    onClick={(event) => event.stopPropagation()}
                  >
                    <ChromeButton
                      variant="ghost"
                      size="sm"
                      disabled={manageAccess.disabled}
                      disabledReason={manageAccess.reason}
                      onClick={() => setEditing(r)}
                    >
                      {t('model_pricing.override')}
                    </ChromeButton>
                    <ChromeButton
                      variant="ghost"
                      size="sm"
                      disabled={manageAccess.disabled || remove.isPending}
                      disabledReason={manageAccess.reason}
                      onClick={() => setRemoving(r)}
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

function UpsertDrawer({
  open,
  initial,
  access,
  onClose,
}: {
  open: boolean;
  initial: modelPricingApi.ModelPrice | null;
  access: ActionAccess;
  onClose: () => void;
}) {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const [provider, setProvider] = React.useState('');
  const [model, setModel] = React.useState('');
  const [promptK, setPromptK] = React.useState('0');
  const [completionK, setCompletionK] = React.useState('0');

  React.useEffect(() => {
    if (open) {
      setProvider(initial?.provider ?? '');
      setModel(initial?.model ?? '');
      setPromptK(String(initial?.prompt_usd_per_1k ?? 0));
      setCompletionK(String(initial?.completion_usd_per_1k ?? 0));
    } else {
      setProvider('');
      setModel('');
      setPromptK('0');
      setCompletionK('0');
    }
  }, [initial, open]);

  const upsert = useMutation({
    mutationFn: () =>
      modelPricingApi.upsert({
        provider,
        model,
        prompt_usd_per_1k: Number(promptK),
        completion_usd_per_1k: Number(completionK),
      }),
    onSuccess: () => {
      toast.success(t('model_pricing.toast_saved'));
      void qc.invalidateQueries({ queryKey: ['model-prices'] });
      onClose();
    },
    onError: (err) => toast.error(toApiError(err).message),
  });
  const promptValue = Number(promptK);
  const completionValue = Number(completionK);
  const invalid =
    provider.trim().length === 0 ||
    model.trim().length === 0 ||
    !Number.isFinite(promptValue) ||
    promptValue < 0 ||
    !Number.isFinite(completionValue) ||
    completionValue < 0;
  const dirty =
    !initial ||
    promptValue !== initial.prompt_usd_per_1k ||
    completionValue !== initial.completion_usd_per_1k;
  const submitDisabled = access.disabled || !dirty;

  return (
    <FormDrawer
      open={open}
      onOpenChange={(v) => !v && onClose()}
      title={t('model_pricing.drawer_title')}
      footer={
        <FormSubmitFooter
          busy={upsert.isPending}
          disabled={submitDisabled}
          invalid={invalid}
          disabledReason={
            access.reason ??
            (invalid
              ? tc('access.form_invalid')
              : !dirty
                ? tc('access.no_changes')
                : undefined)
          }
          onCancel={onClose}
          submitLabel={t('model_pricing.submit_label')}
          formId="model-price-form"
        />
      }
    >
      <form
        id="model-price-form"
        onSubmit={(e) => {
          e.preventDefault();
          if (submitDisabled || invalid || upsert.isPending) return;
          upsert.mutate();
        }}
      >
        <FormSection>
          <FormField label={t('model_pricing.field_provider')} required>
            <FormInput
              value={provider}
              onChange={(e) => setProvider(e.target.value)}
              readOnly={initial !== null}
              disabled={access.disabled || upsert.isPending}
              disabledReason={access.reason}
              required
            />
          </FormField>
          <FormField label={t('model_pricing.field_model')} required>
            <FormInput
              value={model}
              onChange={(e) => setModel(e.target.value)}
              readOnly={initial !== null}
              disabled={access.disabled || upsert.isPending}
              disabledReason={access.reason}
              required
            />
          </FormField>
          <FormField label={t('model_pricing.field_prompt_k')} required>
            <FormInput
              type="number"
              step="0.000001"
              min={0}
              value={promptK}
              onChange={(e) => setPromptK(e.target.value)}
              disabled={access.disabled || upsert.isPending}
              disabledReason={access.reason}
              required
            />
          </FormField>
          <FormField label={t('model_pricing.field_completion_k')} required>
            <FormInput
              type="number"
              step="0.000001"
              min={0}
              value={completionK}
              onChange={(e) => setCompletionK(e.target.value)}
              disabled={access.disabled || upsert.isPending}
              disabledReason={access.reason}
              required
            />
          </FormField>
        </FormSection>
      </form>
    </FormDrawer>
  );
}
