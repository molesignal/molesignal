import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';

import type * as usersApi from '@/api/users';
import { formatRelativeMicros } from '@/routes/pipelines/presentation';
import { Pill, type PillTone } from '@/shell/chrome';
import { Avatar, AvatarFallback, AvatarImage } from '@/shell/ui/avatar';

export type StatusFilter =
  | 'all'
  | 'active'
  | 'pending'
  | 'disabled'
  | 'rejected';

export function UserAvatar({ user }: { user: usersApi.UserView }) {
  const fallback = initials(user.display_name || user.email);
  return (
    <Avatar className="h-8 w-8 border border-bd-0">
      {user.avatar_url && <AvatarImage src={user.avatar_url} alt="" />}
      <AvatarFallback className="bg-indigo-dim font-sans text-type-micro font-bold text-indigo-soft">
        {fallback}
      </AvatarFallback>
    </Avatar>
  );
}

export function UserStatusPill({ user }: { user: usersApi.UserView }) {
  const { t } = useTranslation('iam');
  const status = normalizedStatus(user);
  const tones: Record<StatusFilter, PillTone> = {
    all: 'neutral',
    active: 'green',
    pending: 'yellow',
    disabled: 'red',
    rejected: 'red',
  };
  return <Pill tone={tones[status]}>{t(`users.status_${status}`)}</Pill>;
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
  method: string,
): string {
  const normalized = method.toLocaleLowerCase();
  return t(`users.login_methods.${normalized}`, {
    defaultValue: method.toUpperCase(),
  });
}

export function lastActiveLabel(
  user: usersApi.UserView,
  currentUserId: string | undefined,
  locale: string,
  t: TFunction<'iam'>,
): string {
  if (user.id === currentUserId) return t('users.just_now');
  if (user.last_active_at_micros) {
    return formatRelativeMicros(user.last_active_at_micros, locale);
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
