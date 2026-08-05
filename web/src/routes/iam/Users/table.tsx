import {
  Eye,
  MoreHorizontal,
  Search,
  ShieldCheck,
  UserCheck,
  UserRoundMinus,
  UserX,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable } from '@/admin';
import type * as rolesApi from '@/api/roles';
import type * as usersApi from '@/api/users';
import {
  restrictActionAccess,
  type ActionAccess,
} from '@/product/actionAccess';
import { Pill } from '@/shell/chrome';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';

import {
  lastActiveLabel,
  loginMethodLabel,
  normalizedStatus,
  type StatusFilter,
  UserAvatar,
  UserStatusPill,
} from './presentation';

export function UsersTable({
  access,
  approvePending,
  currentUserId,
  locale,
  removePending,
  roles,
  rows,
  statusPending,
  onApprove,
  onChangeRole,
  onChangeStatus,
  onRemove,
  onView,
}: {
  access: ActionAccess;
  approvePending: boolean;
  currentUserId: string | undefined;
  locale: string;
  removePending: boolean;
  roles: rolesApi.IamRole[];
  rows: usersApi.UserView[];
  statusPending: boolean;
  onApprove: (user: usersApi.UserView) => void;
  onChangeRole: (user: usersApi.UserView) => void;
  onChangeStatus: (user: usersApi.UserView) => void;
  onRemove: (user: usersApi.UserView) => void;
  onView: (user: usersApi.UserView) => void;
}) {
  const { t } = useTranslation('iam');
  const [query, setQuery] = React.useState('');
  const [roleFilter, setRoleFilter] = React.useState('all');
  const [statusFilter, setStatusFilter] = React.useState<StatusFilter>('all');
  const filteredRows = React.useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    return rows.filter((user) => {
      const matchesQuery =
        !needle ||
        [user.display_name, user.email, ...(user.team_names ?? [])]
          .join(' ')
          .toLocaleLowerCase()
          .includes(needle);
      const matchesRole =
        roleFilter === 'all' ||
        user.roles.some((role) => role.id === roleFilter);
      const status = normalizedStatus(user);
      return (
        matchesQuery &&
        matchesRole &&
        (statusFilter === 'all' || status === statusFilter)
      );
    });
  }, [query, roleFilter, rows, statusFilter]);
  const activeCount = rows.filter(
    (user) => normalizedStatus(user) === 'active',
  ).length;
  const pendingCount = rows.filter(
    (user) => normalizedStatus(user) === 'pending',
  ).length;

  return (
    <div className="space-y-4">
      <p className="font-sans text-xs text-tx-2" aria-live="polite">
        {t('users.summary', {
          total: rows.length,
          active: activeCount,
          pending: pendingCount,
        })}
      </p>

      <div className="flex flex-col gap-2 border-y border-bd-0 py-3 sm:flex-row sm:items-center">
        <label className="flex h-9 min-w-0 flex-1 items-center gap-2 rounded-md border border-bd-1 bg-bg-1 px-3 sm:max-w-md">
          <Search className="h-3.5 w-3.5 shrink-0 text-tx-3" />
          <span className="sr-only">{t('users.search_aria')}</span>
          <input
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t('users.search_placeholder')}
            className="min-w-0 flex-1 bg-transparent font-sans text-base text-tx-0 placeholder:text-tx-3 focus:outline-none lg:text-sm"
          />
        </label>
        <label>
          <span className="sr-only">{t('users.role_filter_aria')}</span>
          <select
            value={roleFilter}
            onChange={(event) => setRoleFilter(event.target.value)}
            className="h-9 w-full rounded-md border border-bd-1 bg-bg-1 px-3 font-sans text-sm text-tx-1 focus:outline-none sm:w-36"
          >
            <option value="all">{t('users.filters.all_roles')}</option>
            {roles.map((role) => (
              <option key={role.id} value={role.id}>
                {role.name}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span className="sr-only">{t('users.status_filter_aria')}</span>
          <select
            value={statusFilter}
            onChange={(event) =>
              setStatusFilter(event.target.value as StatusFilter)
            }
            className="h-9 w-full rounded-md border border-bd-1 bg-bg-1 px-3 font-sans text-sm text-tx-1 focus:outline-none sm:w-36"
          >
            <option value="all">{t('users.filters.all_statuses')}</option>
            <option value="active">{t('users.status_active')}</option>
            <option value="pending">{t('users.status_pending')}</option>
            <option value="disabled">{t('users.status_disabled')}</option>
            <option value="rejected">{t('users.status_rejected')}</option>
          </select>
        </label>
      </div>

      {filteredRows.length === 0 ? (
        <div className="flex h-36 flex-col items-center justify-center rounded-md border border-dashed border-bd-1 bg-bg-1 px-6 text-center">
          <Search className="mb-2 h-5 w-5 text-tx-3" />
          <div className="font-sans text-sm font-strong text-tx-1">
            {t('users.no_results_title')}
          </div>
          <div className="mt-1 font-sans text-xs text-tx-3">
            {t('users.no_results_description')}
          </div>
        </div>
      ) : (
        <div className="overflow-hidden rounded-md border border-bd-0 bg-bg-1">
          <DataTable
            rows={filteredRows}
            rowKey={(user) => user.id}
            columns={[
              {
                key: 'user',
                header: t('users.columns.user'),
                width: 210,
                cell: (user) => (
                  <div className="flex min-w-0 items-center gap-2.5">
                    <UserAvatar user={user} />
                    <div className="min-w-0">
                      <div className="truncate text-tx-0">
                        {user.display_name || user.email}
                      </div>
                      <div className="flex items-center gap-1.5 type-micro">
                        {user.is_root && (
                          <span className="text-indigo-soft">
                            {t('users.root_user')}
                          </span>
                        )}
                        {user.id === currentUserId && (
                          <span className="text-tx-3">
                            {t('users.current_user')}
                          </span>
                        )}
                      </div>
                    </div>
                  </div>
                ),
              },
              {
                key: 'email',
                header: t('users.columns.email'),
                width: 240,
                cell: (user) => user.email,
              },
              {
                key: 'role',
                header: t('users.columns.role'),
                width: 110,
                cell: (user) =>
                  user.roles.length ? (
                    <Pill tone="neutral">
                      {user.roles.map((role) => role.name).join(', ')}
                    </Pill>
                  ) : (
                    '—'
                  ),
              },
              {
                key: 'teams',
                header: t('users.columns.teams'),
                width: 190,
                cell: (user) =>
                  user.team_names?.length
                    ? user.team_names.join(', ')
                    : t('users.no_team'),
              },
              {
                key: 'login_method',
                header: t('users.columns.login_method'),
                width: 130,
                cell: (user) => loginMethodLabel(t, user.login_method),
              },
              {
                key: 'last_active',
                header: t('users.columns.last_active'),
                width: 140,
                cell: (user) =>
                  lastActiveLabel(user, currentUserId, locale, t),
              },
              {
                key: 'status',
                header: t('users.columns.status'),
                width: 110,
                cell: (user) => <UserStatusPill user={user} />,
              },
              {
                key: 'actions',
                header: t('users.columns.actions'),
                width: 170,
                headerClassName: 'text-center',
                className: 'overflow-visible text-center',
                cell: (user) => (
                  <UserActions
                    access={access}
                    approvePending={approvePending}
                    currentUserId={currentUserId}
                    removePending={removePending}
                    statusPending={statusPending}
                    user={user}
                    onApprove={onApprove}
                    onChangeRole={onChangeRole}
                    onChangeStatus={onChangeStatus}
                    onRemove={onRemove}
                    onView={onView}
                  />
                ),
              },
            ]}
          />
        </div>
      )}
    </div>
  );
}

function UserActions({
  access,
  approvePending,
  currentUserId,
  removePending,
  statusPending,
  user,
  onApprove,
  onChangeRole,
  onChangeStatus,
  onRemove,
  onView,
}: {
  access: ActionAccess;
  approvePending: boolean;
  currentUserId: string | undefined;
  removePending: boolean;
  statusPending: boolean;
  user: usersApi.UserView;
  onApprove: (user: usersApi.UserView) => void;
  onChangeRole: (user: usersApi.UserView) => void;
  onChangeStatus: (user: usersApi.UserView) => void;
  onRemove: (user: usersApi.UserView) => void;
  onView: (user: usersApi.UserView) => void;
}) {
  const { t } = useTranslation('iam');
  const isSelf = user.id === currentUserId;
  const roleAccess = restrictActionAccess(
    access,
    !isSelf && !user.is_root,
    isSelf ? t('users.current_user_read_only') : t('users.root_user_read_only'),
  );
  const statusAccess = restrictActionAccess(
    access,
    !isSelf && !user.is_root && !statusPending,
    isSelf
      ? t('users.current_user_status_locked')
      : user.is_root
        ? t('users.root_user_read_only')
        : t('users.action_pending'),
  );
  const approveAccess = restrictActionAccess(
    access,
    normalizedStatus(user) === 'pending' && !approvePending,
    normalizedStatus(user) !== 'pending'
      ? t('users.not_pending')
      : t('users.action_pending'),
  );
  const removeAccess = restrictActionAccess(
    access,
    !isSelf && !user.is_root && !removePending,
    isSelf
      ? t('users.current_user_remove_locked')
      : user.is_root
        ? t('users.actions.root_cannot_remove')
        : t('users.action_pending'),
  );

  return (
    <div className="flex items-center justify-center gap-1">
      <button
        type="button"
        onClick={() => onView(user)}
        className="rounded-md px-2 py-1 font-sans text-xs font-strong text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo"
      >
        {t('users.actions.details')}
      </button>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            aria-label={t('users.actions.open_menu', {
              user: user.display_name || user.email,
            })}
            className="flex h-8 w-8 items-center justify-center rounded-md text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo"
          >
            <MoreHorizontal className="h-4 w-4" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-48 border-bd-1 bg-bg-1">
          <DropdownMenuItem onSelect={() => onView(user)}>
            <Eye className="h-4 w-4" />
            {t('users.actions.details')}
          </DropdownMenuItem>
          <DropdownMenuItem
            disabled={roleAccess.disabled}
            disabledReason={roleAccess.reason}
            onSelect={() => roleAccess.allowed && onChangeRole(user)}
          >
            <ShieldCheck className="h-4 w-4" />
            {t('users.actions.change_role')}
          </DropdownMenuItem>
          <DropdownMenuItem
            disabled={approveAccess.disabled}
            disabledReason={approveAccess.reason}
            onSelect={() => approveAccess.allowed && onApprove(user)}
          >
            <UserCheck className="h-4 w-4" />
            {t('users.approve')}
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem
            disabled={statusAccess.disabled}
            disabledReason={statusAccess.reason}
            onSelect={() => statusAccess.allowed && onChangeStatus(user)}
          >
            <UserX className="h-4 w-4" />
            {t(
              user.disabled ? 'users.actions.enable' : 'users.actions.disable',
            )}
          </DropdownMenuItem>
          <DropdownMenuItem
            disabled={removeAccess.disabled}
            disabledReason={removeAccess.reason}
            onSelect={() => removeAccess.allowed && onRemove(user)}
            className="text-red-soft focus:text-red-soft"
          >
            <UserRoundMinus className="h-4 w-4" />
            {t(
              user.is_root
                ? 'users.actions.root_cannot_remove'
                : 'users.actions.remove_member',
            )}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
