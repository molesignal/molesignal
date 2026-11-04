import {
  Eye,
  KeyRound,
  MoreHorizontal,
  Search,
  ShieldCheck,
  UserCheck,
  UserRoundMinus,
  UserX,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable, type DataTableColumn, TableExportButton, useTableSelection } from '@/admin';
import type * as rolesApi from '@/api/roles';
import type * as usersApi from '@/api/users';
import {
  restrictActionAccess,
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { ChromeButton, Pill } from '@/shell/chrome';
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
  UserStatusWord,
} from './presentation';
import { UserBulkBar } from './UserBulkBar';

export function UsersTable({
  access,
  approvePending,
  currentUserId,
  loading = false,
  locale,
  removePending,
  roles,
  rows,
  statusPending,
  onApprove,
  onChangeRole,
  onChangeStatus,
  onRemove,
  onResetMfa,
  onView,
}: {
  access: ActionAccess;
  approvePending: boolean;
  currentUserId: string | undefined;
  /** The list is on its way: its head and its filters stay, nothing in them is counted yet. */
  loading?: boolean;
  locale: string;
  removePending: boolean;
  roles: rolesApi.IamRole[];
  rows: usersApi.UserView[];
  statusPending: boolean;
  onApprove: (user: usersApi.UserView) => void;
  onChangeRole: (user: usersApi.UserView) => void;
  onChangeStatus: (user: usersApi.UserView) => void;
  onRemove: (user: usersApi.UserView) => void;
  onResetMfa: (user: usersApi.UserView) => void;
  onView: (user: usersApi.UserView) => void;
}) {
  const { t } = useTranslation('iam');
  const { t: tc } = useTranslation('common');
  const [query, setQuery] = React.useState('');
  const [roleFilter, setRoleFilter] = React.useState('all');
  const [statusFilter, setStatusFilter] = React.useState<StatusFilter>('all');
  const filteredRows = React.useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    return rows.filter((user) => {
      const matchesQuery =
        !needle ||
        [user.displayName, user.email, ...(user.teamNames ?? [])]
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
  // Nobody can lock themselves out of their own account, and the root user is read-only here.
  const { selection, selectedRows, clear: clearSelection } = useTableSelection(
    filteredRows,
    (user) => user.id,
    (user) => !user.isRoot && user.id !== currentUserId,
  );
  const clearFilters = () => {
    setQuery('');
    setRoleFilter('all');
    setStatusFilter('all');
  };
  const columns: DataTableColumn<usersApi.UserView>[] = [
    {
      key: 'user',
      header: t('users.columns.user'),
      width: 180,
      sortable: true,
      value: (user) => user.displayName || user.email,
      cell: (user) => (
        <div className="flex min-w-0 items-center gap-2.5">
          <UserAvatar user={user} />
          <div className="min-w-0">
            <div className="truncate text-tx-0">
              {user.displayName || user.email}
            </div>
            <div className="flex items-center gap-1.5 type-micro">
              {user.isRoot && (
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
      width: 200,
      sortable: true,
      value: (user) => user.email,
      cell: (user) => user.email,
    },
    {
      key: 'role',
      header: t('users.columns.role'),
      width: 100,
      sortable: true,
      value: (user) => user.roles.map((role) => role.name).join(', '),
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
      width: 180,
      value: (user) => user.teamNames?.join(', ') ?? '',
      headerClassName: 'hidden 2xl:table-cell',
      className: 'hidden 2xl:table-cell',
      cell: (user) =>
        user.teamNames?.length
          ? user.teamNames.join(', ')
          : t('users.no_team'),
    },
    {
      key: 'login_method',
      header: t('users.columns.login_method'),
      width: 100,
      value: (user) => loginMethodLabel(t, user.loginMethod),
      headerClassName: 'hidden xl:table-cell',
      className: 'hidden xl:table-cell',
      cell: (user) => loginMethodLabel(t, user.loginMethod),
    },
    {
      key: 'mfa',
      header: t('users.mfa.column'),
      width: 112,
      sortable: true,
      value: (user) => (user.mfaEnabled ? t('users.mfa.on') : t('users.mfa.off')),
      headerClassName: 'hidden lg:table-cell',
      className: 'hidden lg:table-cell',
      cell: (user) =>
        user.mfaEnabled ? (
          <span className="text-[var(--green-ink)]">{t('users.mfa.on')}</span>
        ) : (
          <span className="text-tx-3">{t('users.mfa.off')}</span>
        ),
    },
    {
      key: 'last_active',
      header: t('users.columns.last_active'),
      width: 110,
      sortable: true,
      value: (user) => lastActiveLabel(user, currentUserId, locale, t),
      sortValue: (user) => user.lastActiveAt ?? null,
      headerClassName: 'hidden xl:table-cell',
      className: 'hidden xl:table-cell',
      cell: (user) =>
        lastActiveLabel(user, currentUserId, locale, t),
    },
    {
      key: 'status',
      header: t('users.columns.status'),
      width: 90,
      sortable: true,
      value: (user) => t(`users.status_${normalizedStatus(user)}`),
      cell: (user) => <UserStatusWord user={user} />,
    },
    {
      key: 'actions',
      header: t('users.columns.actions'),
      width: 110,
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
          onResetMfa={onResetMfa}
          onView={onView}
        />
      ),
    },
  ];

  return (
    <div className="space-y-2">
      <div
        data-iam-filter-surface
        className="flex flex-col gap-2 rounded-md bg-[var(--functional-surface)] p-2 [box-shadow:var(--shadow-functional-surface)] md:flex-row md:items-center"
      >
        <p
          className="shrink-0 font-sans text-xs text-tx-2"
          aria-live="polite"
        >
          {!loading &&
            t('users.summary', {
              total: rows.length,
              active: activeCount,
              pending: pendingCount,
            })}
        </p>
        <div className="flex min-w-0 flex-1 flex-col gap-2 sm:flex-row sm:items-center md:justify-end">
          <label className="flex h-9 min-w-0 flex-1 items-center gap-2 rounded-md bg-[var(--control-surface)] px-3 sm:max-w-sm">
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
              className="h-9 w-full rounded-md border-0 bg-[var(--control-surface)] px-3 font-sans text-sm text-tx-1 focus:outline-none sm:w-36"
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
              className="h-9 w-full rounded-md border-0 bg-[var(--control-surface)] px-3 font-sans text-sm text-tx-1 focus:outline-none sm:w-36"
            >
              <option value="all">{t('users.filters.all_statuses')}</option>
              <option value="active">{t('users.status_active')}</option>
              <option value="pending">{t('users.status_pending')}</option>
              <option value="disabled">{t('users.status_disabled')}</option>
              <option value="rejected">{t('users.status_rejected')}</option>
            </select>
          </label>
          <TableExportButton name="molesignal-users" columns={columns} rows={filteredRows} size="md" />
        </div>
      </div>

      {!loading && filteredRows.length === 0 ? (
        <div className="flex h-36 flex-col items-center justify-center rounded-md bg-[var(--control-surface)] px-6 text-center">
          <Search className="mb-2 h-5 w-5 text-tx-3" />
          <div className="font-sans text-sm font-strong text-tx-1">
            {t('users.no_results_title')}
          </div>
          <div className="mt-1 font-sans text-xs text-tx-3">
            {t('users.no_results_description')}
          </div>
          <ChromeButton size="sm" className="mt-3" onClick={clearFilters}>
            {tc('table.clear_filters')}
          </ChromeButton>
        </div>
      ) : (
        <div className="min-w-0 overflow-hidden">
          {selectedRows.length > 0 && <UserBulkBar users={selectedRows} onClear={clearSelection} />}
          <DataTable
            rows={filteredRows}
            rowKey={(user) => user.id}
            loading={loading}
            {...(access.allowed ? { selection } : {})}
            columns={columns}
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
  onResetMfa,
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
  onResetMfa: (user: usersApi.UserView) => void;
  onView: (user: usersApi.UserView) => void;
}) {
  const canApprove = useActionAccess({ permission: 'org.members.approve' });
  const canResetMfa = useActionAccess({ permission: 'org.members.mfa_reset' });
  const canRemove = useActionAccess({ permission: 'org.members.remove' });
  const { t } = useTranslation('iam');
  const isSelf = user.id === currentUserId;
  const roleAccess = restrictActionAccess(
    access,
    !isSelf && !user.isRoot,
    isSelf ? t('users.current_user_read_only') : t('users.root_user_read_only'),
  );
  const statusAccess = restrictActionAccess(
    access,
    !isSelf && !user.isRoot && !statusPending,
    isSelf
      ? t('users.current_user_status_locked')
      : user.isRoot
        ? t('users.root_user_read_only')
        : t('users.action_pending'),
  );
  const approveAccess = restrictActionAccess(
    canApprove,
    normalizedStatus(user) === 'pending' && !approvePending,
    normalizedStatus(user) !== 'pending'
      ? t('users.not_pending')
      : t('users.action_pending'),
  );
  const resetMfaAccess = restrictActionAccess(
    canResetMfa,
    !isSelf && !user.isRoot && Boolean(user.mfaEnabled),
    isSelf
      ? t('users.mfa.self_reset')
      : user.isRoot
        ? t('users.mfa.root_reset')
        : t('users.mfa.not_enabled'),
  );
  const removeAccess = restrictActionAccess(
    canRemove,
    !isSelf && !user.isRoot && !removePending,
    isSelf
      ? t('users.current_user_remove_locked')
      : user.isRoot
        ? t('users.actions.root_cannot_remove')
        : t('users.action_pending'),
  );

  return (
    <div className="flex items-center justify-center gap-1">
      <button
        type="button"
        onClick={() => onView(user)}
        className="inline-flex h-8 items-center rounded-md px-2 font-sans text-xs font-strong text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 focus-visible:outline-none"
      >
        {t('users.actions.details')}
      </button>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            aria-label={t('users.actions.open_menu', {
              user: user.displayName || user.email,
            })}
            className="flex h-8 w-8 items-center justify-center rounded-md text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 focus-visible:outline-none"
          >
            <MoreHorizontal className="h-4 w-4" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-48">
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
          <DropdownMenuItem
            disabled={resetMfaAccess.disabled}
            disabledReason={resetMfaAccess.reason}
            onSelect={() => resetMfaAccess.allowed && onResetMfa(user)}
          >
            <KeyRound className="h-4 w-4" />
            {t('users.mfa.reset')}
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
              user.isRoot
                ? 'users.actions.root_cannot_remove'
                : 'users.actions.remove_member',
            )}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
