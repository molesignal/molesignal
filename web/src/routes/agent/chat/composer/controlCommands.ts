/**
 * The words control commands take, read into what the composer holds. A word
 * that is not one of them reads as null, so the composer can say which words
 * there are.
 */
import { AGENT_TIME_PRESETS } from '@/investigation/agentContext';

import type { ChatMode, ExecutionPolicy } from '../workspace/types';

/** `/mode` words: the backend lists these as the command's choices. */
const MODE_WORDS: Record<string, ChatMode> = {
  auto: 'auto',
  quick: 'quick',
  deep: 'deep',
  query: 'query_only',
};

/** `/policy` words. */
const POLICY_WORDS: Record<string, ExecutionPolicy> = {
  advice: 'advice_only',
  readonly: 'read_only',
  approval: 'policy',
  auto: 'auto',
  bypass: 'bypass_permissions',
};

const word = (value: string) => value.trim().toLowerCase();

export function modeFor(value: string): ChatMode | null {
  return MODE_WORDS[word(value)] ?? null;
}

export function policyFor(value: string): ExecutionPolicy | null {
  return POLICY_WORDS[word(value)] ?? null;
}

/** A time range preset of the composer: `15m`, `1h`, `7d`… */
export function timePresetFor(value: string): string | null {
  const preset = word(value);
  return preset in AGENT_TIME_PRESETS ? preset : null;
}

/**
 * The automatic choice: the value of the model, profile and prompt selects
 * when nothing is picked, and the word `/model`, `/profile` and `/prompt` take
 * for it.
 */
export const AUTO = 'auto';

/** The word `/team` takes for a personal chat. */
export const PERSONAL = 'personal';

/**
 * The item an argument names, by one of its names or its id and ignoring case;
 * `auto` for the automatic choice, null when nothing has that name. The first
 * name wins over the others: a model provider is named before its model.
 */
export function pickByName<T extends { id: string }>(
  items: readonly T[],
  namesOf: (item: T) => readonly string[],
  value: string,
): T | typeof AUTO | null {
  const wanted = word(value);
  if (wanted === AUTO) return AUTO;
  const named = (position: number) =>
    items.find((item) => {
      const name = namesOf(item)[position];
      return name !== undefined && word(name) === wanted;
    });
  const most = Math.max(0, ...items.map((item) => namesOf(item).length));
  for (let position = 0; position < most; position += 1) {
    const item = named(position);
    if (item) return item;
  }
  return items.find((item) => item.id.toLowerCase() === wanted) ?? null;
}
