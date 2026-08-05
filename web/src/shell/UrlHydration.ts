import pako from 'pako';

import type { GlobalFilter } from '@/stores/useFiltersStore';
import { useFiltersStore } from '@/stores/useFiltersStore';
import type { Frame } from '@/stores/useInvestigationStack';
import { useInvestigationStack } from '@/stores/useInvestigationStack';
import type { TimeWindow } from '@/stores/useTimeStore';
import { useTimeStore } from '@/stores/useTimeStore';

/**
 * Read URL search params and write them into the global stores. Call this in
 * the route loader for any route that participates in shared state, before
 * the route element renders, to avoid a flash of pre-hydration content.
 */
export function hydrateFromSearchParams(search: URLSearchParams): void {
  const t = search.get('time');
  if (t) {
    const [from, to] = t.split('..');
    if (from && to) {
      const mode: TimeWindow['mode'] = from.startsWith('now') || to.startsWith('now') ? 'relative' : 'absolute';
      useTimeStore.getState().setWindow({ from, to, mode });
    }
  } else {
    // Cross-signal links use explicit `from` / `to` params so the URL remains
    // readable and interoperable with external tools. Hydrate that shape too;
    // useSyncStateToUrl will additionally persist the canonical `time` form.
    const from = search.get('from');
    const to = search.get('to');
    if (from && to) {
      const mode: TimeWindow['mode'] = from.startsWith('now') || to.startsWith('now') ? 'relative' : 'absolute';
      useTimeStore.getState().setWindow({ from, to, mode });
    }
  }

  const a = search.get('anchor');
  if (a) {
    useTimeStore.getState().setAnchor({ at: a });
  } else if (search.has('anchor')) {
    useTimeStore.getState().clearAnchor();
  }

  const s = search.get('stack');
  if (s) {
    try {
      const frames = decodeStack(s);
      useInvestigationStack.getState().hydrate(frames);
    } catch {
      // ignore malformed stack — keep current state
    }
  }

  const f = search.get('filters');
  if (f) {
    try {
      useFiltersStore.getState().setAll(decodeFilters(f));
    } catch {
      // ignore malformed filters — keep current state
    }
  } else if (search.has('filters')) {
    useFiltersStore.getState().clearFilters();
  }
}

export function encodeFilters(filters: GlobalFilter[]): string {
  return JSON.stringify(filters.map((f) => [f.key, f.operator ?? '=', f.value]));
}

export function decodeFilters(payload: string): GlobalFilter[] {
  const arr = JSON.parse(payload) as unknown;
  if (!Array.isArray(arr)) return [];
  const filters: GlobalFilter[] = [];
  for (const item of arr) {
    if (!Array.isArray(item)) continue;
    // Backward-compatible with the original `[key, value]` URL payload.
    if (typeof item[0] === 'string' && typeof item[1] === 'string' && item.length === 2) {
      filters.push({ key: item[0], value: item[1], operator: '=' });
      continue;
    }
    if (
      typeof item[0] === 'string'
      && (item[1] === '=' || item[1] === '!=')
      && typeof item[2] === 'string'
    ) {
      filters.push({ key: item[0], operator: item[1], value: item[2] });
    }
  }
  return filters;
}

export function encodeStack(frames: Frame[]): string {
  const minimal = frames.map((f) => ({
    k: f.kind,
    p: f.params,
    t: f.time_range_override,
    a: f.anchor_override,
    pp: f.parent_frame_id,
    pin: f.pinned,
  }));
  const json = JSON.stringify(minimal);
  const deflated = pako.deflate(new TextEncoder().encode(json));
  return base64UrlEncode(deflated);
}

export function decodeStack(payload: string): Frame[] {
  const bytes = base64UrlDecode(payload);
  const json = new TextDecoder().decode(pako.inflate(bytes));
  const arr = JSON.parse(json) as Array<{
    k: Frame['kind'];
    p: Record<string, unknown>;
    t?: TimeWindow;
    a?: string;
    pp?: string;
    pin?: boolean;
  }>;
  // Regenerate ids on decode; consumers don't depend on stable ids across links.
  return arr.map((x, i) => ({
    id: `f${i}`,
    kind: x.k,
    params: x.p,
    ...(x.t !== undefined && { time_range_override: x.t }),
    ...(x.a !== undefined && { anchor_override: x.a }),
    ...(x.pp !== undefined && { parent_frame_id: x.pp }),
    pinned: x.pin ?? false,
    created_at: 0,
  }));
}

function base64UrlEncode(bytes: Uint8Array): string {
  let bin = '';
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function base64UrlDecode(s: string): Uint8Array {
  const pad = s.length % 4;
  const padded = s.replace(/-/g, '+').replace(/_/g, '/') + (pad ? '='.repeat(4 - pad) : '');
  const bin = atob(padded);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}
