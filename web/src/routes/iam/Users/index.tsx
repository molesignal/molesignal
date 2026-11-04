import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog } from '@/admin';
import * as mfaApi from '@/api/mfa';
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
    permission: 'org.members.update',
  });
  const inviteAccess = useActionAccess({ permission: 'org.members.invite' });
  const removeAccess = useActionAccess({ permission: 'org.members.remove' });
  const resetMfaAccess = useActionAccess({ permission: 'org.members.mfa_reset' });
  const [inviting, setInviting] = React.useState(false);
  const [detailUser, setDetailUser] =
    React.useState<usersApi.UserView | null>(null);
  const [roleUser, setRoleUser] =
    React.useState<usersApi.UserView | null>(null);
  const [statusChange, setStatusChange] =
    React.useState<StatusChange | null>(null);
  const [removing, setRemoving] =
    React.useState<usersApi.UserView | null>(null);
  const [resettingMfa, setResettingMfa] =
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
          disabled={inviteAccess.disabled}
          disabledReason={inviteAccess.reason}
          onClick={() => inviteAccess.allowed && setInviting(true)}
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
      if (!auth?.orgId) throw new Error('Missing active organization');
      return usersApi.removeMembership(auth.orgId, userId);
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
  const resetMfa = useMutation({
    mutationFn: (user: usersApi.UserView) => mfaApi.resetMember(user.id),
    onSuccess: (_data, user) => {
      toast.success(
        t('users.mfa.toast_reset', { user: user.displayName || user.email }),
      );
      void refreshUsers();
      setResettingMfa(null);
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
      if (!auth?.orgId) throw new Error('Missing active organization');
      return usersApi.upsertMembership(auth.orgId, {
        userId: user.id,
        roleIds: roleIds,
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
            disabled={inviteAccess.disabled}
            disabledReason={inviteAccess.reason}
            onClick={() => inviteAccess.allowed && setInviting(true)}
          >
            <Plus className="h-3.5 w-3.5" />
            {t('users.invite')}
          </ChromeButton>
        }
        state={pageState?.variant === 'loading' ? null : pageState}
      >
        <UsersTable
          access={manageAccess}
          approvePending={approve.isPending}
          currentUserId={auth?.userId}
          loading={usersQuery.isLoading}
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
          onResetMfa={setResettingMfa}
          onView={setDetailUser}
        />
      </IamListPage>

      <InviteDrawer
        access={inviteAccess}
        open={inviting}
        onClose={() => setInviting(false)}
      />
      <UserDetailDrawer
        user={detailUser}
        currentUserId={auth?.userId}
        locale={i18n.language}
        onClose={() => setDetailUser(null)}
      />
      <RoleDrawer
        access={
          roleUser
            ? restrictActionAccess(
                manageAccess,
                roleUser.id !== auth?.userId && !roleUser.isRoot,
                roleUser.id === auth?.userId
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
          { user: statusChange?.user.displayName || statusChange?.user.email },
        )}
        confirmLabel={t(
          statusChange?.disabled
            ? 'users.actions.disable'
            : 'users.actions.enable',
        )}
        busy={updateStatus.isPending}
        disabled={
          manageAccess.disabled ||
          statusChange?.user.id === auth?.userId ||
          Boolean(statusChange?.user.isRoot)
        }
        disabledReason={
          manageAccess.reason ??
          (statusChange?.user.id === auth?.userId
            ? t('users.current_user_status_locked')
            : statusChange?.user.isRoot
              ? t('users.root_user_read_only')
              : undefined)
        }
        onConfirm={() => {
          if (
            manageAccess.allowed &&
            statusChange &&
            statusChange.user.id !== auth?.userId &&
            !statusChange.user.isRoot
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
          user: removing?.displayName || removing?.email,
        })}
        confirmLabel={t('users.actions.remove_member')}
        busy={removeMembership.isPending}
        disabled={
          removeAccess.disabled ||
          removing?.id === auth?.userId ||
          Boolean(removing?.isRoot)
        }
        disabledReason={
          removeAccess.reason ??
          (removing?.id === auth?.userId
            ? t('users.current_user_remove_locked')
            : removing?.isRoot
              ? t('users.actions.root_cannot_remove')
              : undefined)
        }
        onConfirm={() => {
          if (
            removeAccess.allowed &&
            removing &&
            removing.id !== auth?.userId &&
            !removing.isRoot
          ) {
            removeMembership.mutate(removing.id);
          }
        }}
      />
      <ConfirmDialog
        open={resettingMfa !== null}
        onOpenChange={(open) => !open && setResettingMfa(null)}
        destructive
        title={t('users.mfa.reset_title', {
          user: resettingMfa?.displayName || resettingMfa?.email,
        })}
        description={t('users.mfa.reset_description')}
        confirmLabel={t('users.mfa.reset')}
        busy={resetMfa.isPending}
        busyLabel={t('users.mfa.resetting')}
        disabled={resetMfaAccess.disabled}
        disabledReason={resetMfaAccess.reason}
        onConfirm={() => {
          if (resetMfaAccess.allowed && resettingMfa) resetMfa.mutate(resettingMfa);
        }}
      />
    </>
  );
}
