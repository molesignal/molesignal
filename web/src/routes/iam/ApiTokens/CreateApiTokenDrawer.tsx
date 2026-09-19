import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as apiTokens from '@/api/apiTokens';
import * as rolesApi from '@/api/roles';
import * as serviceAccountsApi from '@/api/serviceAccounts';
import { writeClipboardText } from '@/lib/clipboard';
import { toApiError } from '@/lib/http';
import {
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
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

export function CreateApiTokenDrawer({
  open,
  access,
  defaultServiceAccountId,
  onClose,
}: {
  open: boolean;
  access: ActionAccess;
  defaultServiceAccountId?: string;
  onClose: () => void;
}) {
  const { t } = useTranslation('iam');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const serviceAccountAccess = useActionAccess({
    permission: 'service_accounts.read',
  });
  const [name, setName] = React.useState('');
  const [roleId, setRoleId] = React.useState('');
  const [serviceAccountId, setServiceAccountId] = React.useState('');
  const [expiresInDays, setExpiresInDays] = React.useState('365');
  const [created, setCreated] =
    React.useState<apiTokens.CreatedApiToken | null>(null);
  const roles = useQuery({
    queryKey: ['iam', 'roles'],
    queryFn: rolesApi.list,
    enabled: open,
  });
  const serviceAccounts = useQuery({
    queryKey: ['iam', 'service-accounts'],
    queryFn: serviceAccountsApi.list,
    enabled: open && serviceAccountAccess.allowed,
  });
  const selectedServiceAccount = (serviceAccounts.data ?? []).find(
    (account) => account.id === serviceAccountId,
  );
  const roleOptions = (roles.data ?? []).filter(
    (role) =>
      role.role_type === 'organization' &&
      role.scope === 'organization' &&
      role.key !== 'rum_client',
  );

  React.useEffect(() => {
    if (!open) return;
    setName('');
    setRoleId('');
    setServiceAccountId(defaultServiceAccountId ?? '');
    setExpiresInDays('365');
    setCreated(null);
  }, [defaultServiceAccountId, open]);

  const create = useMutation({
    mutationFn: () => {
      const payload: apiTokens.CreateApiTokenPayload = {
        name: name.trim(),
      };
      if (serviceAccountId) {
        payload.service_account_id = serviceAccountId;
      } else if (roleId) {
        payload.role_id = roleId;
      }
      const trimmedDays = expiresInDays.trim();
      if (trimmedDays) payload.expires_in_days = Number(trimmedDays);
      return apiTokens.create(payload);
    },
    onSuccess: async (token) => {
      setCreated(token);
      toast.success(t('api_tokens.toast_created'));
      await queryClient.invalidateQueries({ queryKey: ['iam', 'api-tokens'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const expiresValue =
    expiresInDays.trim() === '' ? null : Number(expiresInDays);
  const invalid =
    !name.trim() ||
    (expiresValue !== null &&
      (!Number.isFinite(expiresValue) ||
        expiresValue < 1 ||
        expiresValue > 1825));

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
      title={t('api_tokens.drawer_title')}
      subtitle={t('api_tokens.drawer_subtitle')}
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
            disabledReason={
              access.reason ??
              (invalid ? tc('access.form_invalid') : undefined)
            }
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
            <CopyIconButton
              type="button"
              onClick={copyToken}
              label={t('api_tokens.copy_token')}
            />
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
            <FormField
              label={t('api_tokens.fields.principal')}
              hint={t('api_tokens.fields.principal_hint')}
            >
              <FormSelect
                value={serviceAccountId}
                onChange={setServiceAccountId}
                disabled={create.isPending}
                options={[
                  { value: '', label: t('api_tokens.current_user') },
                  ...(serviceAccounts.data ?? [])
                    .filter((account) => !account.disabled)
                    .map((account) => ({
                      value: account.id,
                      label: account.name,
                    })),
                ]}
              />
            </FormField>
            <FormField
              label={t('api_tokens.fields.role')}
              hint={selectedServiceAccount ? t('api_tokens.fields.role_inherited') : ''}
            >
              <FormSelect
                value={
                  selectedServiceAccount
                    ? selectedServiceAccount.role_id
                    : roleId
                }
                onChange={setRoleId}
                disabled={create.isPending || Boolean(selectedServiceAccount)}
                options={
                  selectedServiceAccount
                    ? [
                        {
                          value: selectedServiceAccount.role_id,
                          label: selectedServiceAccount.role_name,
                        },
                      ]
                    : [
                        {
                          value: '',
                          label: t('api_tokens.fields.default_role'),
                        },
                        ...roleOptions.map((role) => ({
                          value: role.id,
                          label: role.name,
                        })),
                      ]
                }
              />
            </FormField>
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
