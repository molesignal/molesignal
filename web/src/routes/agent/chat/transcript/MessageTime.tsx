import { useTranslation } from 'react-i18next';

const MS_PER_DAY = 86_400_000;

function startOfDay(at: Date): number {
  return new Date(at.getFullYear(), at.getMonth(), at.getDate()).getTime();
}

/** Whole calendar days between a moment and now, in the reader's time zone. */
export function daysAgo(at: Date, now: Date): number {
  return Math.round((startOfDay(now) - startOfDay(at)) / MS_PER_DAY);
}

/**
 * When a message was written: the time of day when it was today, otherwise how
 * many days ago; the full date and time when pointed at.
 */
export function MessageTime({ micros, className }: { micros: number; className?: string }) {
  const { t, i18n } = useTranslation('agent');
  const at = new Date(micros / 1000);
  if (Number.isNaN(at.getTime())) return null;
  const days = daysAgo(at, new Date());
  const label =
    days <= 0
      ? at.toLocaleTimeString(i18n.language, { hour: '2-digit', minute: '2-digit' })
      : t('message_time.days_ago', { count: days });
  const full = new Intl.DateTimeFormat(i18n.language, {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(at);
  return (
    <time dateTime={at.toISOString()} title={full} className={className}>
      {label}
    </time>
  );
}
