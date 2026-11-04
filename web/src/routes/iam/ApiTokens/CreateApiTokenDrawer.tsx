import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as apiTokens from '@/api/apiTokens';
import * as serviceAccountsApi from '@/api/serviceAccounts';
import { writeClipboardText } from '@/lib/clipboard';
import { toApiError } from '@/lib/http';
import type { ActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { CopyIconButton } from '@/shell/CopyIconButton';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import {
  PRINCIPAL_SCOPE,
  type ScopeChoice,
  type TokenPrincipal,
  TokenScopeField,
  scopeIncomplete,
  scopePayload,
} from './scope';

/** Where each kind of token is listed, refreshed once a new one is issued. */
export const TOKEN_LIST_KEYS: Record<TokenPrincipal, readonly string[]> = {
  me: ['me', 'api-tokens'],
  service_account: ['iam', 'api-tokens'],
};

/**
 * Issues a token: one of the signed-in user's own, or one for a service
 * account. Either way it never does more than its principal may do, and its
 * secret is shown this once.
 */
export function CreateApiTokenDrawer({
  open,
  access,
  principal,
  defaultServiceAccountId,
  onClose,
}: {
  open: boolean;
  access: ActionAccess;
  principal: TokenPrincipal;
  defaultServiceAccountId?: string;
  onClose: () => void;
}) {
  const { t } = useTranslation('iam');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const [name, setName] = React.useState('');
  const [serviceAccountId, setServiceAccountId] = React.useState('');
  const [scope, setScope] = React.useState<ScopeChoice>(PRINCIPAL_SCOPE);
  const [expiresInDays, setExpiresInDays] = React.useState('365');
  const [created, setCreated] = React.useState<apiTokens.IssuedApiToken | null>(null);
  const forServiceAccount = principal === 'service_account';
  const serviceAccounts = useQuery({
    queryKey: ['iam', 'service-accounts'],
    queryFn: serviceAccountsApi.list,
    enabled: open && forServiceAccount,
  });

  React.useEffect(() => {
    if (!open) return;
    setName('');
    setServiceAccountId(defaultServiceAccountId ?? '');
    setScope(PRINCIPAL_SCOPE);
    setExpiresInDays('365');
    setCreated(null);
  }, [defaultServiceAccountId, open]);

  const create = useMutation({
    mutationFn: () => {
      const trimmedDays = expiresInDays.trim();
      const payload: apiTokens.CreateApiTokenPayload = {
        name: name.trim(),
        ...scopePayload(scope),
        ...(trimmedDays ? { expiresInDays: Number(trimmedDays) } : {}),
      };
      return forServiceAccount
        ? apiTokens.createForServiceAccount({
            ...payload,
            serviceAccountId: serviceAccountId,
          })
        : apiTokens.createMine(payload);
    },
    onSuccess: async (token) => {
      setCreated(token);
      toast.success(t('api_tokens.toast_created'));
      await queryClient.invalidateQueries({ queryKey: TOKEN_LIST_KEYS[principal] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const expiresValue = expiresInDays.trim() === '' ? null : Number(expiresInDays);
  const invalid =
    !name.trim() ||
    (forServiceAccount && !serviceAccountId) ||
    scopeIncomplete(scope) ||
    (expiresValue !== null &&
      (!Number.isFinite(expiresValue) || expiresValue < 1 || expiresValue > 1825));

  const copyToken = async () => {
    if (!created) return;
    try {
      await writeClipboardText(created.token);
      toast.success(t('api_tokens.toast_copied'));
    } catch (error) {
      toast.error(toApiError(error).message);
    }
  };

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={t(`api_tokens.drawer_title.${principal}`)}
      subtitle={t(`api_tokens.drawer_subtitle.${principal}`)}
      footer={
        created ? (
          <ChromeButton variant="primary" onClick={onClose}>
            {t('api_tokens.done')}
          </ChromeButton>
        ) : (
          <FormSubmitFooter
            busy={create.isPending}
            disabled={access.disabled}
            invalid={invalid}
            disabledReason={access.reason ?? (invalid ? tc('access.form_invalid') : undefined)}
            onCancel={onClose}
            submitLabel={t('api_tokens.submit_label')}
            formId="api-token-form"
          />
        )
      }
    >
      {created ? (
        <FormSection
          title={t('api_tokens.created_title')}
          description={t('api_tokens.created_description')}
        >
          <div className="flex min-w-0 items-center gap-2 rounded-md border border-bd-0 bg-bg-2 p-2">
            <code className="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap text-xs text-tx-0">
              {created.token}
            </code>
            <CopyIconButton type="button" onClick={copyToken} label={t('api_tokens.copy_token')} />
          </div>
        </FormSection>
      ) : (
        <form
          id="api-token-form"
          onSubmit={(event) => {
            event.preventDefault();
            if (!invalid && access.allowed) create.mutate();
          }}
        >
          <FormSection>
            <FormField label={t('api_tokens.fields.name')} required>
              <FormInput
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder={t('api_tokens.fields.name_placeholder')}
                disabled={create.isPending}
                required
              />
            </FormField>
            {forServiceAccount && (
              <FormField
                label={t('api_tokens.fields.service_account')}
                hint={t('api_tokens.fields.service_account_hint')}
                required
              >
                <FormSelect
                  value={serviceAccountId}
                  onChange={setServiceAccountId}
                  disabled={create.isPending}
                  options={[
                    { value: '', label: t('api_tokens.fields.service_account_placeholder') },
                    ...(serviceAccounts.data ?? [])
                      .filter((account) => !account.disabled)
                      .map((account) => ({ value: account.id, label: account.name })),
                  ]}
                />
              </FormField>
            )}
          </FormSection>
          <TokenScopeField
            value={scope}
            onChange={setScope}
            principal={principal}
            disabled={create.isPending}
          />
          <FormSection>
            <FormField
              label={t('api_tokens.fields.expires_in_days')}
              hint={t('api_tokens.fields.expires_hint')}
            >
              <FormInput
                type="number"
                min={1}
                max={1825}
                value={expiresInDays}
                onChange={(event) => setExpiresInDays(event.target.value)}
                disabled={create.isPending}
              />
            </FormField>
          </FormSection>
        </form>
      )}
    </FormDrawer>
  );
}
