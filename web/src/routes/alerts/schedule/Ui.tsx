import type { LucideIcon } from 'lucide-react';
import * as React from 'react';

import { cn } from '@/shell/lib/cn';
import {
  Avatar,
  AvatarFallback,
  AvatarImage,
} from '@/shell/ui/avatar';
import type { UserLite } from '@/shell/useUsers';

type AccentTone = 'indigo' | 'green' | 'blue' | 'orange' | 'red';

const ACCENT: Record<AccentTone, string> = {
  indigo: 'bg-indigo-dim text-indigo-soft',
  green: 'bg-green-dim text-green-soft',
  blue: 'bg-blue-dim text-blue-soft',
  orange: 'bg-orange-dim text-orange-soft',
  red: 'bg-red-dim text-red-soft',
};

export function ScheduleSummaryCard({
  icon: Icon,
  label,
  value,
  hint,
  tone = 'indigo',
  onClick,
}: {
  icon: LucideIcon;
  label: React.ReactNode;
  value: React.ReactNode;
  hint: React.ReactNode;
  tone?: AccentTone;
  onClick?: () => void;
}) {
  const content = (
    <>
      <span
        className={cn(
          'grid h-10 w-10 shrink-0 place-items-center rounded-lg',
          ACCENT[tone],
        )}
      >
        <Icon className="h-5 w-5" />
      </span>
      <span className="min-w-0">
        <span className="block type-label font-sans font-strong text-tx-2">
          {label}
        </span>
        <span className="mt-1 flex min-w-0 items-baseline gap-1.5">
          <span className="truncate font-sans text-lg font-display-strong leading-tight tabular-nums text-tx-0 2xl:text-2xl">
            {value}
          </span>
        </span>
        <span className="mt-1 block truncate type-caption font-sans text-tx-3">
          {hint}
        </span>
      </span>
    </>
  );

  const className =
    'flex min-h-[104px] w-full items-center gap-4 rounded-lg border border-bd-0 bg-bg-1 px-4 py-3 text-left';

  if (onClick) {
    return (
      <button
        type="button"
        onClick={onClick}
        className={cn(
          className,
          'transition-colors duration-fast hover:border-bd-1 hover:bg-bg-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo',
        )}
      >
        {content}
      </button>
    );
  }
  return <div className={className}>{content}</div>;
}

export function ScheduleCard({
  title,
  action,
  children,
  className,
  bodyClassName,
}: {
  title: React.ReactNode;
  action?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  bodyClassName?: string;
}) {
  return (
    <section
      className={cn(
        'overflow-hidden rounded-lg border border-bd-0 bg-bg-1',
        className,
      )}
    >
      <header className="flex min-h-11 items-center gap-3 border-b border-bd-0 px-4">
        <h2 className="type-section-title font-sans font-display text-tx-0">
          {title}
        </h2>
        {action && <div className="ml-auto">{action}</div>}
      </header>
      <div className={cn('p-4', bodyClassName)}>{children}</div>
    </section>
  );
}

export function UserAvatar({
  user,
  size = 'md',
  online,
  muted,
}: {
  user?: UserLite | null | undefined;
  size?: 'sm' | 'md' | 'lg';
  online?: boolean;
  muted?: boolean;
}) {
  const dimensions = {
    sm: 'h-6 w-6',
    md: 'h-8 w-8',
    lg: 'h-11 w-11',
  }[size];
  const initials = (user?.name || '?')
    .trim()
    .split(/\s+/)
    .slice(0, 2)
    .map((part) => part[0])
    .join('')
    .toUpperCase();
  return (
    <span className="relative inline-flex shrink-0">
      <Avatar
        className={cn(
          dimensions,
          'border border-bd-0 bg-bg-2',
          muted && 'opacity-45 grayscale',
        )}
      >
        {user?.avatarUrl && (
          <AvatarImage src={user.avatarUrl} alt={user.name} />
        )}
        <AvatarFallback className="bg-bg-3 font-sans font-strong text-tx-2">
          {initials}
        </AvatarFallback>
      </Avatar>
      {online && (
        <span className="absolute -bottom-0.5 -right-0.5 h-2.5 w-2.5 rounded-full border-2 border-bg-1 bg-green" />
      )}
    </span>
  );
}

export function formatScheduleDateTime(
  micros: number,
  locale: string,
  timeZone?: string,
): string {
  return new Intl.DateTimeFormat(locale, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hourCycle: 'h23',
    ...(timeZone ? { timeZone } : {}),
  }).format(new Date(micros / 1000));
}

export function formatScheduleDay(
  micros: number,
  locale: string,
  timeZone?: string,
): string {
  return new Intl.DateTimeFormat(locale, {
    month: 'numeric',
    day: 'numeric',
    weekday: 'short',
    ...(timeZone ? { timeZone } : {}),
  })
    .format(new Date(micros / 1000))
    .replace(/(周[日一二三四五六])/, ' $1');
}

export function formatScheduleTime(
  micros: number,
  locale: string,
  timeZone?: string,
): string {
  return new Intl.DateTimeFormat(locale, {
    hour: '2-digit',
    minute: '2-digit',
    hourCycle: 'h23',
    ...(timeZone ? { timeZone } : {}),
  }).format(new Date(micros / 1000));
}

export function relativeDuration(
  targetMicros: number,
  nowMicros: number,
  locale: string,
): string {
  const diff = targetMicros - nowMicros;
  const absMinutes = Math.max(0, Math.round(Math.abs(diff) / 60_000_000));
  const days = Math.floor(absMinutes / (24 * 60));
  const hours = Math.floor((absMinutes % (24 * 60)) / 60);
  const minutes = absMinutes % 60;
  const parts: string[] = [];
  const zh = locale.toLowerCase().startsWith('zh');
  if (days > 0) parts.push(zh ? `${days} 天` : `${days}d`);
  if (hours > 0) parts.push(zh ? `${hours} 小时` : `${hours}h`);
  if (days === 0 && minutes > 0) {
    parts.push(zh ? `${minutes} 分钟` : `${minutes}m`);
  }
  const value = parts.join(' ') || (zh ? '不到 1 分钟' : '<1m');
  if (diff >= 0) return zh ? `${value}后` : `in ${value}`;
  return zh ? `${value}前` : `${value} ago`;
}
