import type { UserNotifyPreference } from '@/api/notify';
import type { Urgency } from '@/types/alerting';

/** A step waits at most a day. */
const MAX_WAIT_MINUTES = 1440;

/**
 * One step of a person's notification order: the method, the minutes it waits (the first step
 * sends at once), and how the IM app pages harder once the message is sent, `null` for not at all.
 * A method comes twice only with a different boost on each step.
 */
export interface Step {
  endpointId: string;
  wait: string;
  boost: string | null;
}

/** The steps of one urgency, in order, as the preference keeps them. */
export function stepsOf(preference: UserNotifyPreference | undefined, urgency: Urgency): Step[] {
  return (preference?.steps ?? [])
    .filter((step) => (step.urgency ?? 'high') === urgency)
    .sort((left, right) => left.stepOrder - right.stepOrder)
    .map((step) => ({
      endpointId: step.endpointId,
      wait: String(Math.round(step.delaySecs / 60)),
      boost: step.boost ?? null,
    }));
}

/** The seconds a step waits, from the minutes typed for it. */
function waitSecs(minutes: string): number {
  return Math.min(MAX_WAIT_MINUTES, Number.parseInt(minutes, 10) || 0) * 60;
}

/** The steps as they are saved: the methods, the seconds each waits and their boosts, side by side. */
export function savedSteps(steps: Step[]): {
  endpointIds: string[];
  delaysSecs: number[];
  boosts: Array<string | null>;
} {
  return {
    endpointIds: steps.map((step) => step.endpointId),
    delaysSecs: steps.map((step, index) => (index === 0 ? 0 : waitSecs(step.wait))),
    boosts: steps.map((step) => step.boost),
  };
}

/** The boosts another step of this method could still take: none first, then those its connector
 * offers, less those its other steps have. `except` is a step whose own boost does not count. */
export function freeBoosts(
  steps: Step[],
  endpointId: string,
  offered: readonly string[],
  except?: number,
): Array<string | null> {
  const taken = new Set(
    steps.filter((step, index) => index !== except && step.endpointId === endpointId).map((step) => step.boost),
  );

  return [null, ...offered].filter((boost) => !taken.has(boost));
}

/** The step a method is added as, with the first boost it is still free to take; `null` when it can
 * be added no more. A list without waits takes each method once: a second step would only follow a
 * failed send of the first. */
export function addedStep(
  steps: Step[],
  endpointId: string,
  offered: readonly string[],
  repeats: boolean,
): Step | null {
  if (!repeats && steps.some((step) => step.endpointId === endpointId)) return null;

  const boost = freeBoosts(steps, endpointId, offered)[0];
  return boost === undefined ? null : { endpointId, wait: '0', boost };
}

/** The steps with one moved a place up (-1) or down (1). */
export function movedStep(steps: Step[], index: number, direction: -1 | 1): Step[] {
  const target = index + direction;
  if (target < 0 || target >= steps.length) return steps;

  const next = [...steps];
  [next[index], next[target]] = [next[target]!, next[index]!];
  return next;
}

/** A key that names a step among the others: no two steps share a method and a boost. */
export function stepKey(step: Step): string {
  return `${step.endpointId}:${step.boost ?? ''}`;
}
