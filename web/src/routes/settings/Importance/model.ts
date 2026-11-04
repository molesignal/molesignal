import type { ImportanceLevels, UrgencyLevel } from '@/api/importance';
import type { Priority, Severity } from '@/types/alerting';

export const MAX_PRIORITY_LEVELS = 9;
export const MAX_URGENCY_LEVELS = 5;
export const BUILT_IN_URGENCIES = ['high', 'low'];
export const SEVERITIES: readonly Severity[] = ['critical', 'error', 'warning', 'info'];

const KEY = /^[a-z][a-z0-9_]{0,31}$/;

/** One more priority level, or one fewer: always P1 to Pn. */
export function withPriorityCount(levels: ImportanceLevels, count: number): ImportanceLevels {
  const next = Math.min(MAX_PRIORITY_LEVELS, Math.max(1, count));
  const priorities = Array.from({ length: next }, (_, index) => {
    const priority = `P${index + 1}` as Priority;
    return levels.priorities.find((level) => level.priority === priority) ?? { priority, name: '', description: '' };
  });

  return { ...levels, priorities };
}

/** A severity goes to one urgency only: putting it in a level takes it out of the others. */
export function giveSeverity(levels: ImportanceLevels, key: string, severity: Severity): ImportanceLevels {
  return {
    ...levels,
    urgencies: levels.urgencies.map((level) => ({
      ...level,
      severities:
        level.key === key
          ? SEVERITIES.filter((item) => item === severity || level.severities.includes(item))
          : level.severities.filter((item) => item !== severity),
    })),
  };
}

/** A new urgency level: it escalates, quiet hours keep it back, and it uses the low steps. */
export function newUrgency(key: string): UrgencyLevel {
  return { key, name: '', escalates: true, quietHours: 'block', steps: 'low', severities: [] };
}

/** Moves an urgency one place up (more urgent) or down. */
export function moveUrgency(levels: ImportanceLevels, index: number, by: -1 | 1): ImportanceLevels {
  const target = index + by;
  if (target < 0 || target >= levels.urgencies.length) return levels;

  const urgencies = [...levels.urgencies];
  const [moved] = urgencies.splice(index, 1);
  if (moved) urgencies.splice(target, 0, moved);
  return { ...levels, urgencies };
}

/** What keeps the server from taking the levels, as a key under `importance.errors`; null when they are fine. */
export function levelsProblem(levels: ImportanceLevels): string | null {
  const keys = levels.urgencies.map((level) => level.key);

  if (keys.some((key) => !KEY.test(key))) return 'key';
  if (new Set(keys).size !== keys.length) return 'key_twice';
  if (!BUILT_IN_URGENCIES.every((key) => keys.includes(key))) return 'built_in';
  if (levels.urgencies.length > MAX_URGENCY_LEVELS) return 'too_many';
  if (SEVERITIES.some((severity) => levels.urgencies.filter((level) => level.severities.includes(severity)).length !== 1)) {
    return 'severities';
  }
  return null;
}
