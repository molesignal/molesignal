const SERIES_COLOR_VARS = [
  '--chart-1',
  '--chart-2',
  '--chart-3',
  '--chart-4',
  '--chart-5',
  '--chart-6',
  '--chart-7',
  '--chart-8',
] as const;

export const TIME_SERIES_COLORS = SERIES_COLOR_VARS.map((name) => `var(${name})`);

/** FNV-1a provides a stable, cheap series-to-palette mapping. */
export function timeSeriesColor(key: string): string {
  return `var(${SERIES_COLOR_VARS[timeSeriesColorIndex(key)]})`;
}

/**
 * Assign a collision-free palette slot while the number of visible series is
 * within the shared palette size. This keeps adjacent series visually
 * distinct without giving up deterministic, identity-based colours.
 */
export function timeSeriesColors(keys: ReadonlyArray<string>): string[] {
  const assigned = new Map<string, number>();
  const used = new Set<number>();

  for (const key of keys) {
    if (assigned.has(key)) continue;
    const preferred = timeSeriesColorIndex(key);
    let index = preferred;
    if (used.size < SERIES_COLOR_VARS.length) {
      while (used.has(index)) {
        index = (index + 1) % SERIES_COLOR_VARS.length;
      }
    }
    assigned.set(key, index);
    used.add(index);
  }

  return keys.map((key) => `var(${SERIES_COLOR_VARS[assigned.get(key)!]})`);
}

function timeSeriesColorIndex(key: string): number {
  let hash = 0x811c9dc5;
  for (let index = 0; index < key.length; index += 1) {
    hash ^= key.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0) % SERIES_COLOR_VARS.length;
}

export function timeSeriesKey(input: {
  id?: string;
  name: string;
  labels?: Readonly<Record<string, string>>;
}): string {
  if (input.id) return input.id;
  const labels = Object.entries(input.labels ?? {})
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([key, value]) => `${key}=${value}`)
    .join(',');
  return labels ? `${input.name}{${labels}}` : input.name;
}

export function resolveCanvasColor(color: string, fallback = '#708090'): string {
  if (typeof document === 'undefined') return fallback;
  const match = /^var\(\s*(--[^),\s]+)(?:\s*,\s*([^)]+))?\s*\)$/.exec(color.trim());
  if (!match) return color;
  const value = getComputedStyle(document.documentElement).getPropertyValue(match[1]!).trim();
  return value || match[2]?.trim() || fallback;
}

export function colorWithAlpha(color: string, alpha: number): string {
  const resolved = resolveCanvasColor(color);
  const clamped = Math.max(0, Math.min(1, alpha));
  if (resolved.startsWith('#')) {
    const hex = resolved.slice(1);
    const normalized =
      hex.length === 3
        ? hex
            .split('')
            .map((part) => `${part}${part}`)
            .join('')
        : hex.slice(0, 6);
    if (/^[0-9a-f]{6}$/i.test(normalized)) {
      const red = parseInt(normalized.slice(0, 2), 16);
      const green = parseInt(normalized.slice(2, 4), 16);
      const blue = parseInt(normalized.slice(4, 6), 16);
      return `rgba(${red}, ${green}, ${blue}, ${clamped})`;
    }
  }
  const rgb = /^rgba?\(([^)]+)\)$/.exec(resolved);
  if (rgb) {
    const channels = rgb[1]!.split(',').slice(0, 3).join(',');
    return `rgba(${channels}, ${clamped})`;
  }
  return resolved;
}
