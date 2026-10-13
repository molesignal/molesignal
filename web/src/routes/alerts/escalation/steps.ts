import type { EscalationStep, EscalationTarget } from '@/types/alerting';

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
      return target.user_id;
    case 'schedule':
      return target.schedule_id;
    case 'team':
      return target.team_id;
  }
}

/** The same target pointing at something else of its kind. */
export function withRefId(target: EscalationTarget, id: string): EscalationTarget {
  switch (target.kind) {
    case 'user':
      return { ...target, user_id: id };
    case 'schedule':
      return { ...target, schedule_id: id };
    case 'team':
      return { ...target, team_id: id };
  }
}

export function emptyTarget(kind: EscalationTarget['kind']): EscalationTarget {
  switch (kind) {
    case 'user':
      return { kind: 'user', user_id: '' };
    case 'schedule':
      return { kind: 'schedule', schedule_id: '' };
    case 'team':
      return { kind: 'team', team_id: '' };
  }
}

export function emptyStep(): EscalationStep {
  return { targets: [emptyTarget('user')], ack_timeout_secs: 300, min_severity: null };
}

/** The names a policy's targets are shown by: people, schedules and teams, each by id. */
export interface TargetNames {
  user: (id: string) => string;
  schedule: (id: string) => string;
  team: (id: string) => string;
}

/** Who a step tells, by name. A target whose name is not known yet is shown by its id. */
export function stepTargetNames(step: EscalationStep, names: TargetNames): string[] {
  return step.targets
    .map((target) => {
      const id = targetRefId(target);

      return id ? names[target.kind](id) : '';
    })
    .filter(Boolean);
}
