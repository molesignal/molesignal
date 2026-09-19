import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { KeyRound, Power, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { ConfirmDialog } from '@/admin';
import * as rolesApi from '@/api/roles';
import * as serviceAccountsApi from '@/api/serviceAccounts';
import { toApiError } from '@/lib/http';
import {
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
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

export function ManageServiceAccountDrawer({
  account,
  access,
  onClose,
}: {
  account: serviceAccountsApi.ServiceAccount | null;
  access: ActionAccess;
  onClose: () => void;
}) {
  const { t } = useTranslation('iam');
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const apiTokenAccess = useActionAccess({ permission: 'api_tokens.read' });
  const [name, setName] = React.useState('');
  const [description, setDescription] = React.useState('');
  const [roleId, setRoleId] = React.useState('');
  const [deleteOpen, setDeleteOpen] = React.useState(false);
  const accountId = account?.id ?? '';
  const roles = useQuery({
    queryKey: ['iam', 'roles'],
    queryFn: rolesApi.list,
    enabled: Boolean(account),
  });
  const roleOptions = (roles.data ?? []).filter(
    (role) =>
      role.role_type === 'organization' &&
      role.scope === 'organization' &&
      role.key !== 'rum_client',
  );

  React.useEffect(() => {
    setName(account?.name ?? '');
    setDescription(account?.description ?? '');
    setRoleId(account?.role_id ?? '');
  }, [account, accountId]);

  const refresh = () =>
    Promise.all([
      queryClient.invalidateQueries({ queryKey: ['iam', 'service-accounts'] }),
      queryClient.invalidateQueries({ queryKey: ['iam', 'api-tokens'] }),
    ]);
  const setDisabled = useMutation({
    mutationFn: () =>
      serviceAccountsApi.setDisabled(accountId, !account?.disabled),
    onSuccess: async () => {
      toast.success(t('service_accounts.toast_status_updated'));
      await refresh();
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const update = useMutation({
    mutationFn: () =>
      serviceAccountsApi.update(accountId, {
        name: name.trim(),
        description: description.trim(),
        role_id: roleId,
      }),
    onSuccess: async () => {
      toast.success(t('service_accounts.toast_updated'));
      await refresh();
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const remove = useMutation({
    mutationFn: () => serviceAccountsApi.remove(accountId),
    onSuccess: async () => {
      toast.success(t('service_accounts.toast_deleted'));
      setDeleteOpen(false);
      await refresh();
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const openApiTokens = () => {
    onClose();
    navigate(
      `/iam/api-tokens?service_account_id=${encodeURIComponent(accountId)}`,
    );
  };

  return (
    <>
      <FormDrawer
        open={Boolean(account)}
        onOpenChange={(next) => !next && onClose()}
        title={account?.name ?? ''}
        subtitle={account?.description || t('service_accounts.manage_subtitle')}
        footer={
          <div className="flex w-full items-center justify-between gap-3">
            <ChromeButton
              className="text-red-soft enabled:hover:bg-red-dim"
              disabled={access.disabled || remove.isPending}
              disabledReason={access.reason}
              onClick={() => setDeleteOpen(true)}
            >
              <Trash2 className="h-3 w-3" />
              {t('service_accounts.delete')}
            </ChromeButton>
            <div className="flex items-center gap-2">
              <ChromeButton
                disabled={access.disabled || setDisabled.isPending}
                disabledReason={access.reason}
                onClick={() => setDisabled.mutate()}
              >
                <Power className="h-3 w-3" />
                {t(
                  `service_accounts.${account?.disabled ? 'enable' : 'disable'}`,
                )}
              </ChromeButton>
              <ChromeButton variant="primary" onClick={onClose}>
                {t('service_accounts.done')}
              </ChromeButton>
            </div>
          </div>
        }
      >
        <FormSection
          title={t('service_accounts.authentication_title')}
          description={t('service_accounts.authentication_description')}
        >
          <div className="rounded-md border border-bd-0 bg-bg-2 p-3">
            <div className="text-xs text-tx-2">
              {t('service_accounts.principal_id')}
            </div>
            <code className="mt-1 block break-all text-xs text-tx-0">
              {accountId}
            </code>
          </div>
          <ChromeButton
            disabled={apiTokenAccess.disabled}
            disabledReason={apiTokenAccess.reason}
            onClick={openApiTokens}
          >
            <KeyRound className="h-3 w-3" />
            {t('service_accounts.manage_api_tokens')}
          </ChromeButton>
        </FormSection>
        <form
          id="update-service-account"
          onSubmit={(event) => {
            event.preventDefault();
            if (name.trim() && roleId && access.allowed) update.mutate();
          }}
        >
          <FormSection title={t('service_accounts.details_title')}>
            <FormField label={t('service_accounts.fields.name')} required>
              <FormInput
                value={name}
                maxLength={128}
                onChange={(event) => setName(event.target.value)}
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
                options={roleOptions.map((role) => ({
                  value: role.id,
                  label: role.name,
                }))}
              />
            </FormField>
            <FormSubmitFooter
              formId="update-service-account"
              busy={update.isPending}
              disabled={access.disabled}
              invalid={!name.trim() || !roleId}
              disabledReason={access.reason}
              onCancel={() => {
                setName(account?.name ?? '');
                setDescription(account?.description ?? '');
                setRoleId(account?.role_id ?? '');
              }}
              submitLabel={t('service_accounts.save_changes')}
            />
          </FormSection>
        </form>
      </FormDrawer>
      <ConfirmDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        destructive
        title={t('service_accounts.delete_confirm_title')}
        description={t('service_accounts.delete_confirm_description')}
        confirmLabel={t('service_accounts.delete')}
        busy={remove.isPending}
        disabled={access.disabled}
        disabledReason={access.reason}
        onConfirm={() => remove.mutate()}
      />
    </>
  );
}
