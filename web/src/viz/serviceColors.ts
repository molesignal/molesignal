import type { PaletteKey } from '@/viz/timeseries/themeAdapter';

const SERVICE_COLOR_KEYS: readonly PaletteKey[] = [
  '--accent',
  '--blue',
  '--green',
  '--yellow',
  '--red',
  '--purple',
  '--primary',
];

export type ServiceKind = 'rust' | 'go' | 'python' | 'db' | 'http' | 'other';

/** Stable service-kind colors for traces, topology, and future pipeline views. */
export const SERVICE_KIND_COLOR_KEY: Record<ServiceKind, PaletteKey> = {
  rust: '--primary',
  go: '--blue',
  python: '--green',
  db: '--yellow',
  http: '--purple',
  other: '--accent',
};

const cache = new Map<string, PaletteKey>();

/** Stable hash-based service → palette key, shared by logs and traces. */
export function colorKeyForService(service: string): PaletteKey {
  const cached = cache.get(service);
  if (cached) return cached;
  let hash = 0;
  for (let index = 0; index < service.length; index += 1) {
    hash = (hash * 31 + service.charCodeAt(index)) | 0;
  }
  const key = SERVICE_COLOR_KEYS[Math.abs(hash) % SERVICE_COLOR_KEYS.length]!;
  cache.set(service, key);
  return key;
}

export function colorKeyForServiceKind(kind: string): PaletteKey {
  return SERVICE_KIND_COLOR_KEY[kind.toLowerCase() as ServiceKind]
    ?? SERVICE_KIND_COLOR_KEY.other;
}
