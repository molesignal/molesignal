/**
 * This browser's copy of an interface preference.
 *
 * The server keeps how a user prefers a page to look, so it follows them to
 * another browser. This copy is what the page opens on before the server has
 * answered, and what stands when the server cannot be reached.
 */
export interface CachedUiPreference<T> {
  value: T;
  /** When it was written here, in milliseconds. */
  at: number;
}

const PREFIX = 'molesignal.ui-preference.v1';

/** Whose preference it is, in which organization, of what purpose, and under what key. */
export interface UiPreferenceAddress {
  userId: string;
  orgId: string;
  scope: string;
  key: string;
}

export function uiPreferenceStorageKey({ userId, orgId, scope, key }: UiPreferenceAddress): string {
  return [PREFIX, userId, orgId, scope, key].map(encodeURIComponent).join(':');
}

export function readCachedUiPreference<T>(
  address: UiPreferenceAddress,
  parse: (raw: unknown) => T | null,
): CachedUiPreference<T> | null {
  try {
    const raw = globalThis.localStorage?.getItem(uiPreferenceStorageKey(address));
    if (!raw) return null;

    const stored = JSON.parse(raw) as { value?: unknown; at?: unknown };
    const value = parse(stored.value);

    return value === null ? null : { value, at: typeof stored.at === 'number' ? stored.at : 0 };
  } catch {
    // Storage that is closed or holds something else is storage with nothing in it.
    return null;
  }
}

export function writeCachedUiPreference<T>(address: UiPreferenceAddress, value: T): void {
  try {
    globalThis.localStorage?.setItem(
      uiPreferenceStorageKey(address),
      JSON.stringify({ value, at: Date.now() } satisfies CachedUiPreference<T>),
    );
  } catch {
    // A full or closed storage loses the copy, not the preference: the server still has it.
  }
}

export function clearCachedUiPreference(address: UiPreferenceAddress): void {
  try {
    globalThis.localStorage?.removeItem(uiPreferenceStorageKey(address));
  } catch {
    // Nothing to clear.
  }
}
