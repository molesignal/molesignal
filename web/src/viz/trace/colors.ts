import type { PaletteKey } from '@/viz/timeseries/themeAdapter';

const SERVICE_COLOR_KEYS: PaletteKey[] = [
  '--accent',
  '--blue',
  '--green',
  '--yellow',
  '--red',
  '--purple',
  '--primary',
];

const cache = new Map<string, PaletteKey>();

/**
 * Stable hash-based service → palette key. Same service always picks the
 * same color across renders / sessions.
 */
export function colorKeyForService(service: string): PaletteKey {
  const cached = cache.get(service);
  if (cached) return cached;
  let h = 0;
  for (let i = 0; i < service.length; i++) {
    h = (h * 31 + service.charCodeAt(i)) | 0;
  }
  const idx = Math.abs(h) % SERVICE_COLOR_KEYS.length;
  const key = SERVICE_COLOR_KEYS[idx]!;
  cache.set(service, key);
  return key;
}
