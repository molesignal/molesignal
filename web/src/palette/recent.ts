import type { ItemKind, ResultItem } from './registry';

const MAX_RECENT = 6;

/** A place in one organization means nothing in another. */
function storageKey(orgId: string): string {
  return `molesignal-palette-recent:${orgId}`;
}

export interface RecentEntry {
  kind: ItemKind;
  id: string;
  label: string;
  subtitle?: string | undefined;
}

/** Places count as somewhere one went back to; a toggle or a "create" does not. */
function isPlace(item: ResultItem): boolean {
  return item.kind !== 'action' || item.id.startsWith('goto:') || item.id.startsWith('route:');
}

function isRecentEntry(value: unknown): value is RecentEntry {
  const entry = value as Partial<RecentEntry> | null;
  return typeof entry?.kind === 'string' && typeof entry.id === 'string' && typeof entry.label === 'string';
}

export function readRecents(orgId: string): RecentEntry[] {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(storageKey(orgId)) ?? '[]');
    return Array.isArray(raw) ? raw.filter(isRecentEntry) : [];
  } catch {
    return [];
  }
}

export function rememberRecent(item: ResultItem, orgId: string): void {
  if (!isPlace(item)) return;

  const entry: RecentEntry = {
    kind: item.kind,
    id: item.id,
    label: item.label,
    ...(item.subtitle !== undefined ? { subtitle: item.subtitle } : {}),
  };
  const others = readRecents(orgId).filter((recent) => !(recent.kind === item.kind && recent.id === item.id));

  try {
    localStorage.setItem(storageKey(orgId), JSON.stringify([entry, ...others].slice(0, MAX_RECENT)));
  } catch {
    // Quota: recents are a convenience.
  }
}
