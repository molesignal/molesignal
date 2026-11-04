import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';

import type * as usersApi from '@/api/users';
import { formatRelativeMicros } from '@/routes/pipelines/presentation';
import { StatusWord, type StatusTone } from '@/shell/StatusWord';
import { Avatar, AvatarFallback, AvatarImage } from '@/shell/ui/avatar';

export type StatusFilter =
  | 'all'
  | 'active'
  | 'pending'
  | 'disabled'
  | 'rejected';

export function UserAvatar({ user }: { user: usersApi.UserView }) {
  const fallback = initials(user.displayName || user.email);
  return (
    <Avatar className="h-8 w-8">
      {user.avatarUrl && <AvatarImage src={user.avatarUrl} alt="" />}
      <AvatarFallback className="bg-indigo-dim font-sans text-type-micro font-bold text-tx-0">
        {fallback}
      </AvatarFallback>
    </Avatar>
  );
}

export function UserStatusWord({ user }: { user: usersApi.UserView }) {
  const { t } = useTranslation('iam');
  const status = normalizedStatus(user);
  const tones: Record<StatusFilter, StatusTone> = {
    all: 'neutral',
    active: 'green',
    pending: 'yellow',
    disabled: 'red',
    rejected: 'red',
  };
  return (
    <StatusWord tone={tones[status]} strong={status !== 'active'}>
      {t(`users.status_${status}`)}
    </StatusWord>
  );
}

export function normalizedStatus(
  user: usersApi.UserView,
): Exclude<StatusFilter, 'all'> {
  if (user.disabled) return 'disabled';
  if (user.status === 'pending') return 'pending';
  if (user.status === 'rejected') return 'rejected';
  return 'active';
}

export function loginMethodLabel(
  t: TFunction<'iam'>,
  method: string | null | undefined,
): string {
  const displayMethod = method?.trim() || 'password';
  const normalized = displayMethod.toLocaleLowerCase();
  return t(`users.login_methods.${normalized}`, {
    defaultValue: displayMethod.toUpperCase(),
  });
}

export function lastActiveLabel(
  user: usersApi.UserView,
  currentUserId: string | undefined,
  locale: string,
  t: TFunction<'iam'>,
): string {
  if (user.id === currentUserId) return t('users.just_now');
  if (user.lastActiveAt) {
    return formatRelativeMicros(user.lastActiveAt, locale);
  }
  if (user.status === 'pending' || user.status === 'rejected') {
    return t('users.never_login');
  }
  return t('users.activity_unrecorded');
}

export function formatAbsoluteMicros(
  micros: number | null | undefined,
  locale: string,
): string {
  if (!micros) return '—';
  return new Intl.DateTimeFormat(locale, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(micros / 1000));
}

function initials(value: string): string {
  const parts = value.trim().split(/\s+/).filter(Boolean);
  if (parts.length > 1) {
    return `${parts[0]?.[0] ?? ''}${parts[1]?.[0] ?? ''}`.toUpperCase();
  }
  return value.trim().slice(0, 2).toUpperCase() || '?';
}
