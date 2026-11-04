/** What a tag's key may hold: lowercase letters, digits and `_ . - /`, as the backend keeps them. */
const TAG_KEY = /^[a-z0-9_./-]{1,64}$/;

export const MAX_TAGS = 32;
export const MAX_TAG_VALUE_LENGTH = 128;
/** The longest Apdex threshold a service may set, in milliseconds: a minute. */
export const MAX_APDEX_MILLIS = 60_000;

/** Tags as people type them: `key=value` or the key alone, a comma between each. */
export function formatTags(tags: Readonly<Record<string, string>> | undefined): string {
  return Object.entries(tags ?? {})
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([key, value]) => (value ? `${key}=${value}` : key))
    .join(', ');
}

export type TagsProblem = 'key' | 'value' | 'too_many' | 'twice';

/**
 * The tags a typed line holds, or what is wrong with it: a key out of shape,
 * a value too long, too many tags, a key given twice. Spaces around keys and
 * values are dropped; an empty line is no tag.
 */
export function parseTags(text: string): { tags: Record<string, string>; problem: TagsProblem | null; at?: string } {
  const tags: Record<string, string> = {};
  const parts = text
    .split(',')
    .map((part) => part.trim())
    .filter(Boolean);

  for (const part of parts) {
    const at = part.indexOf('=');
    const key = (at < 0 ? part : part.slice(0, at)).trim();
    const value = at < 0 ? '' : part.slice(at + 1).trim();

    if (!TAG_KEY.test(key)) return { tags, problem: 'key', at: key || part };
    if (value.length > MAX_TAG_VALUE_LENGTH) return { tags, problem: 'value', at: key };
    if (key in tags) return { tags, problem: 'twice', at: key };
    tags[key] = value;
  }

  if (Object.keys(tags).length > MAX_TAGS) return { tags, problem: 'too_many' };
  return { tags, problem: null };
}

/** An Apdex threshold typed in milliseconds, in microseconds; `null` while empty, the default then. */
export function parseApdexMillis(text: string): { micros: number | null; valid: boolean } {
  const trimmed = text.trim();
  if (!trimmed) return { micros: null, valid: true };

  const millis = Number(trimmed);
  if (!Number.isFinite(millis) || millis <= 0 || millis > MAX_APDEX_MILLIS) return { micros: null, valid: false };

  return { micros: Math.round(millis * 1_000), valid: true };
}

/** A kept threshold as the field shows it, in milliseconds. */
export function apdexMillisText(micros: number | null | undefined): string {
  return micros ? String(micros / 1_000) : '';
}

const ADDRESS_URL = /^(?:https?|ssh|git|git\+ssh|ssh\+git):\/\/([^/?#]*)/i;
const ADDRESS_SCP = /^[A-Za-z0-9._-]+(?::[^@\s]*)?@[A-Za-z0-9][A-Za-z0-9.-]*:(?!\/\/).+$/;
const HOST = /^(?:[A-Za-z0-9][A-Za-z0-9.-]*|\[[0-9A-Fa-f:.]+\])$/;

/**
 * Whether a typed repository is an address the backend keeps: http(s), ssh or
 * git, or git's `user@host:path`. Empty is no repository, which is fine.
 */
export function isRepositoryAddress(text: string): boolean {
  const value = text.trim();
  if (!value) return true;
  if (/\s/.test(value) || value.length > 2_048) return false;

  const url = ADDRESS_URL.exec(value);
  if (url) return HOST.test((url[1] ?? '').slice((url[1] ?? '').lastIndexOf('@') + 1).replace(/:\d+$/, ''));

  return ADDRESS_SCP.test(value);
}
