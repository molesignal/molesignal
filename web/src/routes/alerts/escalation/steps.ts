import type { EscalationStep, EscalationTarget, Schedule } from '@/types/alerting';

export const ACK_TIMEOUTS = [60, 300, 600, 900, 1800, 3600] as const;

/** A wait as it is written beside a step: `5m`, `1h`, or seconds when it is neither. */
export function secsLabel(secs: number): string {
  if (secs > 0 && secs % 3600 === 0) return `${secs / 3600}h`;
  if (secs > 0 && secs % 60 === 0) return `${secs / 60}m`;

  return `${secs}s`;
}

/** The id of what a target points at, whichever kind it is. */
export function targetRefId(target: EscalationTarget): string {
  switch (target.kind) {
    case 'user':
      return target.userId;
    case 'schedule':
      return target.scheduleId;
    case 'team':
      return target.teamId;
  }
}

/** The same target pointing at something else of its kind. */
export function withRefId(target: EscalationTarget, id: string): EscalationTarget {
  switch (target.kind) {
    case 'user':
      return { ...target, userId: id };
    case 'schedule':
      return { ...target, scheduleId: id };
    case 'team':
      return { ...target, teamId: id };
  }
}

export function emptyTarget(kind: EscalationTarget['kind']): EscalationTarget {
  switch (kind) {
    case 'user':
      return { kind: 'user', userId: '' };
    case 'schedule':
      return { kind: 'schedule', scheduleId: '' };
    case 'team':
      return { kind: 'team', teamId: '' };
  }
}

export function emptyStep(): EscalationStep {
  return { targets: [emptyTarget('user')], ackTimeoutSecs: 300, minSeverity: null };
}

/** The names a policy's targets are shown by: people, schedules and teams, each by id. */
export interface TargetNames {
  user: (id: string) => string;
  schedule: (id: string) => string;
  team: (id: string) => string;
}

/** Who a step tells, by what `names` calls each target. */
export function stepTargetNames(step: EscalationStep, names: TargetNames): string[] {
  return step.targets
    .map((target) => {
      const id = targetRefId(target);

      return id ? names[target.kind](id) : '';
    })
    .filter(Boolean);
}

/**
 * The people a step can page: the people it names and everyone who takes turns in its schedules.
 * A team is left out: its group route may reach it even when its members have nothing set up.
 */
export function stepPeople(step: EscalationStep, schedules: ReadonlyMap<string, Schedule>): string[] {
  const people = new Set<string>();
  for (const target of step.targets) {
    if (target.kind === 'user' && target.userId) people.add(target.userId);
    if (target.kind === 'schedule') {
      for (const rotation of schedules.get(target.scheduleId)?.rotations ?? []) {
        for (const member of rotation.members) people.add(member);
      }
    }
  }
  return [...people];
}
