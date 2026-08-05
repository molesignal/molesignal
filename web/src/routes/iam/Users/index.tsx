import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog } from '@/admin';
import * as rolesApi from '@/api/roles';
import * as usersApi from '@/api/users';
import { toApiError } from '@/lib/http';
import {
  restrictActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';
import { useAuthStore } from '@/stores/auth';

import { IamListPage } from '../IamLayout';
import {
  InviteDrawer,
  RoleDrawer,
  UserDetailDrawer,
} from './drawers';
import { UsersTable } from './table';

interface StatusChange {
  user: usersApi.UserView;
  disabled: boolean;
}

export function Users() {
  const { t, i18n } = useTranslation('iam');
  const qc = useQueryClient();
  const auth = useAuthStore((state) => state.ctx);
  const manageAccess = useActionAccess({
    permission: 'org.members.manage',
  });
  const [inviting, setInviting] = React.useState(false);
  const [detailUser, setDetailUser] =
    React.useState<usersApi.UserView | null>(null);
  const [roleUser, setRoleUser] =
    React.useState<usersApi.UserView | null>(null);
  const [statusChange, setStatusChange] =
    React.useState<StatusChange | null>(null);
  const [removing, setRemoving] =
    React.useState<usersApi.UserView | null>(null);

  const usersQuery = useQuery({
    queryKey: ['iam', 'users'],
    queryFn: () => usersApi.list(),
  });
  const rolesQuery = useQuery({
    queryKey: ['iam', 'roles'],
    queryFn: () => rolesApi.list(),
  });
  const rows = React.useMemo(() => usersQuery.data ?? [], [usersQuery.data]);
  const pageState = productStateFor(
    queryStateFor({
      isLoading: usersQuery.isLoading,
      isError: usersQuery.isError,
      data: rows,
    }),
    {
      error: usersQuery.error,
      emptyTitle: t('users.empty_title'),
      emptyDescription: t('users.empty_description'),
      emptyAction: (
        <ChromeButton
          variant="primary"
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          onClick={() => manageAccess.allowed && setInviting(true)}
        >
          <Plus className="h-3.5 w-3.5" />
          {t('users.invite')}
        </ChromeButton>
      ),
    },
  );
  const refreshUsers = () =>
    qc.invalidateQueries({ queryKey: ['iam', 'users'] });

  const removeMembership = useMutation({
    mutationFn: (userId: string) => {
      if (!auth?.org_id) throw new Error('Missing active organization');
      return usersApi.removeMembership(auth.org_id, userId);
    },
    onSuccess: () => {
      toast.success(t('users.toast_removed'));
      void refreshUsers();
      setRemoving(null);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const approve = useMutation({
    mutationFn: (id: string) => usersApi.approve(id),
    onSuccess: () => {
      toast.success(t('users.toast_approved'));
      void refreshUsers();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const updateStatus = useMutation({
    mutationFn: ({ user, disabled }: StatusChange) =>
      usersApi.update(user.id, { disabled }),
    onSuccess: (_data, variables) => {
      toast.success(
        t(variables.disabled ? 'users.toast_disabled' : 'users.toast_enabled'),
      );
      void refreshUsers();
      setStatusChange(null);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const updateRole = useMutation({
    mutationFn: ({
      user,
      roleIds,
    }: {
      user: usersApi.UserView;
      roleIds: string[];
    }) => {
      if (!auth?.org_id) throw new Error('Missing active organization');
      return usersApi.upsertMembership(auth.org_id, {
        user_id: user.id,
        role_ids: roleIds,
      });
    },
    onSuccess: () => {
      toast.success(t('users.toast_role_updated'));
      void refreshUsers();
      setRoleUser(null);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <>
      <IamListPage
        title={t('users.title')}
        subtitle={t('users.subtitle') as string}
        toolbar={
          <ChromeButton
            variant="primary"
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
            onClick={() => manageAccess.allowed && setInviting(true)}
          >
            <Plus className="h-3.5 w-3.5" />
            {t('users.invite')}
          </ChromeButton>
        }
        state={pageState}
      >
        <UsersTable
          access={manageAccess}
          approvePending={approve.isPending}
          currentUserId={auth?.user_id}
          locale={i18n.language}
          removePending={removeMembership.isPending}
          roles={rolesQuery.data ?? []}
          rows={rows}
          statusPending={updateStatus.isPending}
          onApprove={(user) => approve.mutate(user.id)}
          onChangeRole={setRoleUser}
          onChangeStatus={(user) =>
            setStatusChange({ user, disabled: !user.disabled })
          }
          onRemove={setRemoving}
          onView={setDetailUser}
        />
      </IamListPage>

      <InviteDrawer
        access={manageAccess}
        open={inviting}
        onClose={() => setInviting(false)}
      />
      <UserDetailDrawer
        user={detailUser}
        currentUserId={auth?.user_id}
        locale={i18n.language}
        onClose={() => setDetailUser(null)}
      />
      <RoleDrawer
        access={
          roleUser
            ? restrictActionAccess(
                manageAccess,
                roleUser.id !== auth?.user_id && !roleUser.is_root,
                roleUser.id === auth?.user_id
                  ? t('users.current_user_read_only')
                  : t('users.root_user_read_only'),
              )
            : manageAccess
        }
        user={roleUser}
        roles={rolesQuery.data ?? []}
        busy={updateRole.isPending}
        onClose={() => setRoleUser(null)}
        onSave={(roleIds) =>
          roleUser && updateRole.mutate({ user: roleUser, roleIds })
        }
      />
      <ConfirmDialog
        open={statusChange !== null}
        onOpenChange={(open) => !open && setStatusChange(null)}
        destructive={Boolean(statusChange?.disabled)}
        title={t(
          statusChange?.disabled
            ? 'users.disable_confirm_title'
            : 'users.enable_confirm_title',
        )}
        description={t(
          statusChange?.disabled
            ? 'users.disable_confirm_description'
            : 'users.enable_confirm_description',
          { user: statusChange?.user.display_name || statusChange?.user.email },
        )}
        confirmLabel={t(
          statusChange?.disabled
            ? 'users.actions.disable'
            : 'users.actions.enable',
        )}
        busy={updateStatus.isPending}
        disabled={
          manageAccess.disabled ||
          statusChange?.user.id === auth?.user_id ||
          Boolean(statusChange?.user.is_root)
        }
        disabledReason={
          manageAccess.reason ??
          (statusChange?.user.id === auth?.user_id
            ? t('users.current_user_status_locked')
            : statusChange?.user.is_root
              ? t('users.root_user_read_only')
              : undefined)
        }
        onConfirm={() => {
          if (
            manageAccess.allowed &&
            statusChange &&
            statusChange.user.id !== auth?.user_id &&
            !statusChange.user.is_root
          ) {
            updateStatus.mutate(statusChange);
          }
        }}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        destructive
        title={t('users.remove_confirm_title')}
        description={t('users.remove_confirm_description', {
          user: removing?.display_name || removing?.email,
        })}
        confirmLabel={t('users.actions.remove_member')}
        busy={removeMembership.isPending}
        disabled={
          manageAccess.disabled ||
          removing?.id === auth?.user_id ||
          Boolean(removing?.is_root)
        }
        disabledReason={
          manageAccess.reason ??
          (removing?.id === auth?.user_id
            ? t('users.current_user_remove_locked')
            : removing?.is_root
              ? t('users.actions.root_cannot_remove')
              : undefined)
        }
        onConfirm={() => {
          if (
            manageAccess.allowed &&
            removing &&
            removing.id !== auth?.user_id &&
            !removing.is_root
          ) {
            removeMembership.mutate(removing.id);
          }
        }}
      />
    </>
  );
}
