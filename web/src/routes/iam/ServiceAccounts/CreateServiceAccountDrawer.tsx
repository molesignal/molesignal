import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as rolesApi from '@/api/roles';
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
  FormTextarea,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

export function CreateServiceAccountDrawer({
  open,
  access,
  onClose,
}: {
  open: boolean;
  access: ActionAccess;
  onClose: () => void;
}) {
  const { t } = useTranslation('iam');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const [name, setName] = React.useState('');
  const [description, setDescription] = React.useState('');
  const [roleId, setRoleId] = React.useState('');
  const [created, setCreated] =
    React.useState<serviceAccountsApi.ProvisionedServiceAccount | null>(null);
  const roles = useQuery({
    queryKey: ['iam', 'roles'],
    queryFn: rolesApi.list,
    enabled: open,
  });
  const roleOptions = (roles.data ?? []).filter(
    (role) =>
      role.role_type === 'organization' &&
      role.scope === 'organization' &&
      role.key !== 'rum_client',
  );

  React.useEffect(() => {
    if (!open) return;
    setName('');
    setDescription('');
    setRoleId('');
    setCreated(null);
  }, [open]);

  const mutation = useMutation({
    mutationFn: () =>
      serviceAccountsApi.create({
        name: name.trim(),
        description: description.trim(),
        role_id: roleId,
      }),
    onSuccess: async (result) => {
      setCreated(result);
      toast.success(t('service_accounts.toast_created'));
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: ['iam', 'service-accounts'],
        }),
        queryClient.invalidateQueries({ queryKey: ['iam', 'api-tokens'] }),
      ]);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const invalid = !name.trim() || !roleId;

  const copyToken = async () => {
    if (!created) return;
    try {
      await writeClipboardText(created.api_token.token);
      toast.success(t('service_accounts.toast_token_copied'));
    } catch (error) {
      toast.error(toApiError(error).message);
    }
  };

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={t('service_accounts.drawer_title')}
      subtitle={t('service_accounts.drawer_subtitle')}
      footer={
        created ? (
          <ChromeButton variant="primary" onClick={onClose}>
            {t('service_accounts.done')}
          </ChromeButton>
        ) : (
          <FormSubmitFooter
            formId="create-service-account-form"
            busy={mutation.isPending}
            disabled={access.disabled}
            invalid={invalid}
            disabledReason={
              access.reason ?? (invalid ? tc('access.form_invalid') : undefined)
            }
            onCancel={onClose}
            submitLabel={t('service_accounts.submit_label')}
          />
        )
      }
    >
      {created ? (
        <FormSection
          title={t('service_accounts.created_title')}
          description={t('service_accounts.created_description')}
        >
          <div className="flex min-w-0 items-center gap-2 rounded-md border border-bd-0 bg-bg-2 p-2">
            <code className="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap text-xs text-tx-0">
              {created.api_token.token}
            </code>
            <CopyIconButton
              type="button"
              onClick={copyToken}
              label={t('service_accounts.copy_initial_token')}
            />
          </div>
        </FormSection>
      ) : (
        <form
          id="create-service-account-form"
          onSubmit={(event) => {
            event.preventDefault();
            if (!invalid && access.allowed) mutation.mutate();
          }}
        >
          <FormSection
            description={t('service_accounts.non_interactive_hint')}
          >
            <FormField label={t('service_accounts.fields.name')} required>
              <FormInput
                value={name}
                maxLength={128}
                onChange={(event) => setName(event.target.value)}
                placeholder={t('service_accounts.fields.name_placeholder')}
              />
            </FormField>
            <FormField label={t('service_accounts.fields.description')}>
              <FormTextarea
                value={description}
                maxLength={2000}
                onChange={(event) => setDescription(event.target.value)}
              />
            </FormField>
            <FormField label={t('service_accounts.fields.role')} required>
              <FormSelect
                value={roleId}
                onChange={setRoleId}
                placeholder={t('service_accounts.fields.select_role')}
                options={roleOptions.map((role) => ({
                  value: role.id,
                  label: role.name,
                }))}
              />
            </FormField>
          </FormSection>
        </form>
      )}
    </FormDrawer>
  );
}
