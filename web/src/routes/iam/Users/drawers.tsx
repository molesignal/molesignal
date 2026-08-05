import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as invitationsApi from '@/api/invitations';
import * as rolesApi from '@/api/roles';
import type * as usersApi from '@/api/users';
import { toApiError } from '@/lib/http';
import type { ActionAccess } from '@/product/actionAccess';
import {
  FormChecklist,
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import {
  formatAbsoluteMicros,
  lastActiveLabel,
  loginMethodLabel,
  UserAvatar,
  UserStatusPill,
} from './presentation';

export function InviteDrawer({
  access,
  open,
  onClose,
}: {
  access: ActionAccess;
  open: boolean;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const { t } = useTranslation('iam');
  const [email, setEmail] = React.useState('');
  const [roleId, setRoleId] = React.useState('');
  const rolesQuery = useQuery({
    queryKey: ['iam', 'roles'],
    queryFn: () => rolesApi.list(),
    enabled: open,
  });

  React.useEffect(() => {
    if (!open) {
      setEmail('');
      setRoleId('');
    }
  }, [open]);

  const create = useMutation({
    mutationFn: () =>
      invitationsApi.create({
        email: email.trim(),
        ...(roleId ? { role_id: roleId } : {}),
      }),
    onSuccess: () => {
      toast.success(t('invitations.toast_invited'));
      void qc.invalidateQueries({ queryKey: ['iam', 'invitations'] });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const invalid = !email.trim() || !email.includes('@');

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={t('invitations.drawer_title')}
      subtitle={t('users.invite_subtitle')}
      width={520}
      footer={
        <FormSubmitFooter
          busy={create.isPending}
          disabled={access.disabled}
          disabledReason={access.reason}
          invalid={invalid}
          onCancel={onClose}
          submitLabel={t('invitations.submit_label')}
          formId="iam-user-invite-form"
        />
      }
    >
      <form
        id="iam-user-invite-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (access.allowed && !invalid) create.mutate();
        }}
      >
        <FormSection>
          <FormField label={t('invitations.fields.email')} required>
            <FormInput
              value={email}
              onChange={(event) => setEmail(event.target.value)}
              type="email"
              placeholder="teammate@example.com"
              required
              disabled={access.disabled || create.isPending}
              disabledReason={access.reason}
            />
          </FormField>
          <FormField label={t('invitations.fields.role')} required>
            <FormSelect
              value={roleId}
              onChange={setRoleId}
              disabled={access.disabled || create.isPending}
              disabledReason={access.reason}
              options={[
                { value: '', label: t('invitations.fields.default_role') },
                ...(rolesQuery.data ?? []).map((role) => ({
                  value: role.id,
                  label: role.name,
                })),
              ]}
            />
          </FormField>
        </FormSection>
      </form>
    </FormDrawer>
  );
}

export function RoleDrawer({
  access,
  user,
  roles,
  busy,
  onClose,
  onSave,
}: {
  access: ActionAccess;
  user: usersApi.UserView | null;
  roles: rolesApi.IamRole[];
  busy: boolean;
  onClose: () => void;
  onSave: (roleIds: string[]) => void;
}) {
  const { t } = useTranslation('iam');
  const [roleIds, setRoleIds] = React.useState<string[]>([]);

  React.useEffect(() => {
    setRoleIds(user?.roles.map((role) => role.id) ?? []);
  }, [user]);

  const initialRoleIds = user?.roles.map((role) => role.id).sort() ?? [];
  const dirty =
    JSON.stringify([...roleIds].sort()) !== JSON.stringify(initialRoleIds);
  const invalid = roleIds.length === 0;

  return (
    <FormDrawer
      open={user !== null}
      onOpenChange={(open) => !open && onClose()}
      title={t('users.role_drawer_title')}
      subtitle={t('users.role_drawer_subtitle', {
        user: user?.display_name || user?.email,
      })}
      width={520}
      footer={
        <FormSubmitFooter
          busy={busy}
          disabled={access.disabled || !dirty}
          disabledReason={
            access.reason ?? (!dirty ? t('users.no_role_changes') : undefined)
          }
          invalid={invalid}
          onCancel={onClose}
          submitLabel={t('users.save_role')}
          formId="iam-user-role-form"
        />
      }
    >
      <form
        id="iam-user-role-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (access.allowed && dirty && !invalid) onSave(roleIds);
        }}
      >
        <FormSection>
          <FormField label={t('users.columns.role')} required>
            <FormChecklist
              selected={roleIds}
              onChange={setRoleIds}
              disabled={access.disabled || busy}
              disabledReason={access.reason}
              options={roles.map((role) => ({
                value: role.id,
                label: role.name,
                hint: role.description,
              }))}
            />
          </FormField>
        </FormSection>
      </form>
    </FormDrawer>
  );
}

export function UserDetailDrawer({
  user,
  currentUserId,
  locale,
  onClose,
}: {
  user: usersApi.UserView | null;
  currentUserId: string | undefined;
  locale: string;
  onClose: () => void;
}) {
  const { t } = useTranslation('iam');
  if (!user) return null;

  const detailRows = [
    { label: t('users.columns.email'), value: user.email },
    {
      label: t('users.columns.role'),
      value: user.roles.length
        ? user.roles.map((role) => role.name).join(', ')
        : '—',
    },
    {
      label: t('users.columns.teams'),
      value: user.team_names?.length
        ? user.team_names.join(', ')
        : t('users.no_team'),
    },
    {
      label: t('users.columns.login_method'),
      value: loginMethodLabel(t, user.login_method),
    },
    {
      label: t('users.columns.last_active'),
      value: lastActiveLabel(user, currentUserId, locale, t),
    },
    {
      label: t('users.columns.joined'),
      value: formatAbsoluteMicros(user.joined_at_micros, locale),
    },
    {
      label: t('users.account_created'),
      value: formatAbsoluteMicros(user.created_at_micros, locale),
    },
  ];

  return (
    <FormDrawer
      open
      onOpenChange={(open) => !open && onClose()}
      title={user.display_name || user.email}
      subtitle={t('users.detail_subtitle')}
      width={560}
    >
      <div className="mb-6 flex items-center gap-3 border-b border-bd-0 pb-5">
        <UserAvatar user={user} />
        <div className="min-w-0 flex-1">
          <div className="truncate font-sans text-sm font-strong text-tx-0">
            {user.email}
          </div>
          <div className="mt-1">
            <UserStatusPill user={user} />
          </div>
        </div>
      </div>
      <dl className="grid grid-cols-[128px_minmax(0,1fr)] gap-x-4 gap-y-4 font-sans text-sm">
        {detailRows.map((row) => (
          <React.Fragment key={row.label}>
            <dt className="text-tx-3">{row.label}</dt>
            <dd className="min-w-0 break-words text-tx-0">{row.value}</dd>
          </React.Fragment>
        ))}
      </dl>
    </FormDrawer>
  );
}
