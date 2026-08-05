import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { PageHeader } from '@/admin';
import * as billingApi from '@/api/billing';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState, productStateFor } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { FormInput, FormSelect } from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import {
  SectionBody,
  SettingsGroupStack,
  SettingsRow,
  SettingsSection,
} from './_atoms';
import { useSettingsSaveStatus } from './SettingsSaveStatus';

export function Billing() {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const saveStatus = useSettingsSaveStatus();
  const manageAccess = useActionAccess({
    permission: 'org.billing.manage',
  });

  const q = useQuery({
    queryKey: ['billing-settings'],
    queryFn: () => billingApi.get(),
    retry: false,
  });

  const [enabled, setEnabled] = React.useState(false);
  const [tolerance, setTolerance] = React.useState('300');
  const [webhookSecret, setWebhookSecret] = React.useState('');
  const [apiKey, setApiKey] = React.useState('');
  const [baseline, setBaseline] = React.useState({
    enabled: false,
    tolerance: '300',
  });
  const hydrated = React.useRef(false);

  React.useEffect(() => {
    if (q.data && !hydrated.current) {
      setEnabled(q.data.enabled);
      setTolerance(String(q.data.signature_tolerance_secs));
      setBaseline({
        enabled: q.data.enabled,
        tolerance: String(q.data.signature_tolerance_secs),
      });
      hydrated.current = true;
    }
  }, [q.data]);

  const save = useMutation({
    mutationFn: () =>
      billingApi.update({
        enabled,
        signature_tolerance_secs: Math.max(1, Number(tolerance) || 300),
        // Blank keeps the existing secret; a typed value replaces it.
        ...(webhookSecret.trim() ? { webhook_secret: webhookSecret.trim() } : {}),
        ...(apiKey.trim() ? { api_key: apiKey.trim() } : {}),
      }),
    onMutate: () => saveStatus.beginSave(),
    onSuccess: (data) => {
      toast.success(t('billing.toast_saved'));
      qc.setQueryData(['billing-settings'], data);
      setBaseline({
        enabled: data.enabled,
        tolerance: String(data.signature_tolerance_secs),
      });
      setEnabled(data.enabled);
      setTolerance(String(data.signature_tolerance_secs));
      setWebhookSecret('');
      setApiKey('');
      saveStatus.completeSave();
    },
    onError: (err) => {
      saveStatus.failSave();
      toast.error(toApiError(err).message);
    },
  });
  const dirty =
    enabled !== baseline.enabled ||
    tolerance !== baseline.tolerance ||
    webhookSecret.trim().length > 0 ||
    apiKey.trim().length > 0;
  const invalid = Number(tolerance) < 1 || !Number.isFinite(Number(tolerance));
  const controlsDisabledReason =
    manageAccess.reason ??
    (save.isPending ? tc('access.operation_pending') : undefined);

  React.useEffect(() => {
    saveStatus.setDraftDirty('billing.settings', dirty);
    return () => saveStatus.setDraftDirty('billing.settings', false);
  }, [dirty, saveStatus]);

  const reset = () => {
    setEnabled(baseline.enabled);
    setTolerance(baseline.tolerance);
    setWebhookSecret('');
    setApiKey('');
  };

  const pageState = productStateFor(
    queryStateFor({
      isLoading: q.isLoading,
      isError: q.isError,
      data: q.data ? [q.data] : [],
    }),
    { error: q.error },
  );

  const secretPlaceholder = (isSet: boolean) =>
    isSet ? t('billing.placeholder_set') : t('billing.placeholder_unset');

  return (
    <>
      <PageHeader title={t('billing.title')} subtitle={t('billing.subtitle') as string} />
      <SectionBody>
        {pageState ? (
          <ProductState {...pageState} />
        ) : (
          <SettingsGroupStack>
            <form
              className="w-full"
              onSubmit={(e) => {
                e.preventDefault();
                if (
                  manageAccess.disabled ||
                  !dirty ||
                  invalid ||
                  save.isPending
                ) {
                  return;
                }
                save.mutate();
              }}
            >
              <SettingsSection
                title={t('billing.section_stripe')}
                className="border-t-0"
              >
                <SettingsRow
                  label={t('billing.fields.enabled')}
                  description={t('billing.hints.enabled')}
                  controlClassName="w-full"
                >
                  <FormSelect
                    value={enabled ? 'on' : 'off'}
                    onChange={(v) => setEnabled(v === 'on')}
                    disabled={manageAccess.disabled || save.isPending}
                    disabledReason={controlsDisabledReason}
                    options={[
                      { value: 'on', label: t('billing.enabled_on') },
                      { value: 'off', label: t('billing.enabled_off') },
                    ]}
                  />
                </SettingsRow>
                <SettingsRow
                  label={t('billing.fields.webhook_secret')}
                  description={t('billing.hints.webhook_secret')}
                  controlClassName="w-full"
                >
                  <FormInput
                    type="password"
                    value={webhookSecret}
                    onChange={(e) => setWebhookSecret(e.target.value)}
                    placeholder={secretPlaceholder(q.data?.webhook_secret_set ?? false)}
                    autoComplete="off"
                    disabled={manageAccess.disabled || save.isPending}
                    disabledReason={controlsDisabledReason}
                  />
                </SettingsRow>
                <SettingsRow
                  label={t('billing.fields.api_key')}
                  description={t('billing.hints.api_key')}
                  controlClassName="w-full"
                >
                  <FormInput
                    type="password"
                    value={apiKey}
                    onChange={(e) => setApiKey(e.target.value)}
                    placeholder={secretPlaceholder(q.data?.api_key_set ?? false)}
                    autoComplete="off"
                    disabled={manageAccess.disabled || save.isPending}
                    disabledReason={controlsDisabledReason}
                  />
                </SettingsRow>
                <SettingsRow
                  label={t('billing.fields.tolerance')}
                  description={t('billing.hints.tolerance')}
                  controlClassName="w-full"
                >
                  <FormInput
                    type="number"
                    min={1}
                    value={tolerance}
                    onChange={(e) => setTolerance(e.target.value)}
                    disabled={manageAccess.disabled || save.isPending}
                    disabledReason={controlsDisabledReason}
                  />
                </SettingsRow>
              </SettingsSection>
              <div className="mt-4 flex justify-end gap-2">
                <ChromeButton
                  type="button"
                  onClick={reset}
                  disabled={manageAccess.disabled || !dirty || save.isPending}
                  disabledReason={
                    manageAccess.reason ??
                    (save.isPending
                      ? tc('access.operation_pending')
                      : !dirty
                        ? tc('access.no_changes')
                        : undefined)
                  }
                >
                  {tc('actions.reset')}
                </ChromeButton>
                <ChromeButton
                  type="submit"
                  variant="primary"
                  disabled={
                    manageAccess.disabled || !dirty || invalid || save.isPending
                  }
                  disabledReason={
                    manageAccess.reason ??
                    (save.isPending
                        ? tc('access.operation_pending')
                        : invalid
                          ? tc('access.form_invalid')
                        : !dirty
                          ? tc('access.no_changes')
                          : undefined)
                  }
                >
                  {save.isPending ? t('billing.saving') : t('billing.save')}
                </ChromeButton>
              </div>
            </form>
          </SettingsGroupStack>
        )}
      </SectionBody>
    </>
  );
}
