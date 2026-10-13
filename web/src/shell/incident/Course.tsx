import { useTranslation } from 'react-i18next';

import { formatMicrosActive } from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import { useUsers } from '@/shell/useUsers';
import type { Incident } from '@/types/alerting';

interface CourseEvent {
  key: 'fired' | 'acknowledged' | 'resolved';
  at: number;
  label: string;
  dot: string;
}

/** A span in the units an incident is timed in: seconds, then minutes, then hours and minutes. */
export function courseDuration(micros: number): string {
  const seconds = Math.max(0, Math.round(micros / 1_000_000));

  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.round(seconds / 60)}m`;

  const hours = Math.floor(seconds / 3600);
  const minutes = Math.round((seconds % 3600) / 60);

  return minutes > 0 ? `${hours}h ${minutes}m` : `${hours}h`;
}

/**
 * What became of an incident, in order: it fired, somebody took it, it
 * recovered. Each step names who did it and how long after the incident fired,
 * and is dated the way the rest of the app dates things.
 */
export function IncidentCourse({ incident }: { incident: Incident }) {
  const { t } = useTranslation('alerts');
  const users = useUsers();
  const by = (key: 'acknowledged' | 'resolved', userId: string | undefined) =>
    userId
      ? t(`drawer.timeline.${key}_by`, { name: users.nameOf(userId) })
      : t(`drawer.timeline.${key}`);
  const events: CourseEvent[] = [
    { key: 'fired', at: incident.created_at, label: t('drawer.timeline.created'), dot: 'bg-red' },
    ...(incident.acknowledged_at
      ? [
          {
            key: 'acknowledged' as const,
            at: incident.acknowledged_at,
            label: by('acknowledged', incident.acknowledged_by),
            dot: 'bg-yellow',
          },
        ]
      : []),
    ...(incident.resolved_at
      ? [
          {
            key: 'resolved' as const,
            at: incident.resolved_at,
            label: by('resolved', incident.resolved_by),
            dot: 'bg-green',
          },
        ]
      : []),
  ];

  return (
    <ol data-incident-course className="flex flex-col">
      {events.map((event, index) => {
        const last = index === events.length - 1;

        return (
          <li key={event.key} className="grid grid-cols-[auto_minmax(0,1fr)_auto] gap-x-3 font-sans text-xs">
            <span aria-hidden="true" className="flex flex-col items-center pt-[5px]">
              <span className={cn('h-2 w-2 shrink-0 rounded-full', event.dot)} />
              {!last && <span className="mt-1 w-px flex-1 bg-bd-1" />}
            </span>
            <span className={cn('min-w-0', !last && 'pb-3')}>
              <span className="text-tx-0">{event.label}</span>
              {index > 0 && (
                <span className="ml-2 tabular-nums text-tx-2">
                  {t('drawer.timeline.after', { duration: courseDuration(event.at - incident.created_at) })}
                </span>
              )}
            </span>
            <time
              dateTime={new Date(Math.floor(event.at / 1000)).toISOString()}
              className="shrink-0 tabular-nums text-tx-2"
            >
              {formatMicrosActive(event.at)}
            </time>
          </li>
        );
      })}
    </ol>
  );
}
