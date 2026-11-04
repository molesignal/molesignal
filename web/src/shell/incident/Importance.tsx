import { useQuery } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';

import * as importanceApi from '@/api/importance';
import { DEFAULT_LEVELS, type ImportanceLevels } from '@/api/importance';
import { cn } from '@/shell/lib/cn';
import type { Incident, Priority, Urgency } from '@/types/alerting';

/** Every priority key the server takes, P1 the heaviest; an organization uses the first few. */
export const ALL_PRIORITIES: readonly Priority[] = ['P1', 'P2', 'P3', 'P4', 'P5', 'P6', 'P7', 'P8', 'P9'];

/** The organization's priority and urgency levels: the built-in ones until they arrive or when they can't be read. */
export function useImportance(): ImportanceLevels {
  const query = useQuery({
    queryKey: ['alerts', 'importance'],
    queryFn: importanceApi.get,
    staleTime: 60_000,
  });

  return query.data?.levels ?? DEFAULT_LEVELS;
}

/** The priority keys of the organization, P1 first. */
export function prioritiesOf(levels: ImportanceLevels): Priority[] {
  return levels.priorities.map((level) => level.priority);
}

/** The urgency keys of the organization, the most urgent first. */
export function urgenciesOf(levels: ImportanceLevels): Urgency[] {
  return levels.urgencies.map((level) => level.key);
}

/** `P2`, or `P2 · High` when the organization named it. */
export function priorityLabel(levels: ImportanceLevels, priority: Priority): string {
  const name = levels.priorities.find((level) => level.priority === priority)?.name;
  return name ? `${priority} · ${name}` : priority;
}

/** The organization's name for an urgency, or the default name of a built-in one, or its key. */
export function urgencyLabel(levels: ImportanceLevels, urgency: Urgency, t: TFunction<'alerts'>): string {
  const name = levels.urgencies.find((level) => level.key === urgency)?.name;
  return name || t(`importance.urgency.${urgency}`, { defaultValue: urgency });
}

const PRIORITY_INK = ['text-[var(--red-ink)]', 'text-[var(--orange-ink)]', 'text-[var(--yellow-ink)]'];

function priorityRank(priority: Priority | null | undefined): number {
  return priority ? Number(priority.slice(1)) : 10;
}

/** Incidents opened before urgency existed count as high, as the server counts them. */
export function urgencyOf(incident: Pick<Incident, 'urgency'>): Urgency {
  return incident.urgency ?? 'high';
}

/** Heavier first; an incident without a priority after every one with one. */
export function byPriority(left: Pick<Incident, 'priority'>, right: Pick<Incident, 'priority'>): number {
  return priorityRank(left.priority) - priorityRank(right.priority);
}

/** A priority as a coloured word, with the organization's name when it gave one; a dash when there is none. */
export function PriorityText({
  priority,
  className,
}: {
  priority: Priority | null | undefined;
  className?: string;
}) {
  const { t } = useTranslation('alerts');
  const levels = useImportance();

  if (!priority) {
    return (
      <span className={cn('text-tx-3', className)} aria-label={t('importance.no_priority')}>
        —
      </span>
    );
  }

  const name = levels.priorities.find((level) => level.priority === priority)?.name;

  return (
    <span
      data-priority={priority}
      title={name || undefined}
      className={cn('font-strong tabular-nums', PRIORITY_INK[priorityRank(priority) - 1] ?? 'text-tx-1', className)}
    >
      {priority}
    </span>
  );
}

/** What an incident says about its urgency: nothing at high, the way incidents page unless something
 * says otherwise; any other level by its name. */
export function urgencyNote(
  levels: ImportanceLevels,
  incident: Pick<Incident, 'urgency'>,
  t: TFunction<'alerts'>,
): string | null {
  const urgency = urgencyOf(incident);
  return urgency === 'high' ? null : urgencyLabel(levels, urgency, t);
}

/** Moving to `next` raises the urgency: the levels run most urgent first; a key the organization no
 * longer has counts as high, as the server counts it. */
export function raisesUrgency(levels: ImportanceLevels, current: Urgency, next: Urgency): boolean {
  const rank = (key: Urgency) => {
    const index = levels.urgencies.findIndex((level) => level.key === key);
    return index < 0 ? levels.urgencies.findIndex((level) => level.key === 'high') : index;
  };
  return rank(next) < rank(current);
}

/** What a level of urgency does, in a few words: whether it escalates, what quiet hours do, which steps it uses. */
export function urgencyHint(levels: ImportanceLevels, urgency: Urgency, t: TFunction<'alerts'>): string {
  const level = levels.urgencies.find((item) => item.key === urgency);
  if (!level) return '';

  return [
    t(level.escalates ? 'importance.behaviour.escalates' : 'importance.behaviour.first_step'),
    t(level.quietHours === 'switch' ? 'importance.behaviour.quiet_switch' : 'importance.behaviour.quiet_block'),
    t(level.steps === 'high' ? 'importance.behaviour.steps_high' : 'importance.behaviour.steps_low'),
  ].join(' · ');
}
