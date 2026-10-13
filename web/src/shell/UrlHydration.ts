import pako from 'pako';

import type { GlobalFilter } from '@/stores/useFiltersStore';
import { useFiltersStore } from '@/stores/useFiltersStore';
import type { FrameContent } from '@/stores/useInvestigationStack';
import { useInvestigationStack } from '@/stores/useInvestigationStack';
import type { Anchor, TimeWindow } from '@/stores/useTimeStore';
import { sameTimeWindow, useTimeStore } from '@/stores/useTimeStore';

/**
 * The time window and anchor a URL names, if it names any. `window` is absent
 * when the URL says nothing about time; `anchor` is `null` when the URL clears
 * it and absent when it does not mention it.
 */
export function timeFromSearchParams(search: URLSearchParams): {
  window?: TimeWindow;
  anchor?: Anchor | null;
} {
  const result: { window?: TimeWindow; anchor?: Anchor | null } = {};
  // The canonical form is `time=from..to`. Cross-signal links use explicit
  // `from` / `to` params so the URL remains readable and interoperable with
  // external tools; useSyncStateToUrl additionally persists the canonical form.
  const time = search.get('time');
  const [from, to] = time ? time.split('..') : [search.get('from'), search.get('to')];
  if (from && to) {
    const mode: TimeWindow['mode'] = from.startsWith('now') || to.startsWith('now') ? 'relative' : 'absolute';
    result.window = { from, to, mode };
  }

  const anchor = search.get('anchor');
  if (anchor) result.anchor = { at: anchor };
  else if (search.has('anchor')) result.anchor = null;
  return result;
}

/**
 * Read URL search params and write them into the global stores. Call this in
 * the route loader for any route that participates in shared state, before
 * the route element renders, to avoid a flash of pre-hydration content.
 *
 * A value the store already holds is left alone: writing an equal copy would
 * hand every reader a new object and re-run the queries that depend on it.
 */
export function hydrateFromSearchParams(search: URLSearchParams): void {
  const time = timeFromSearchParams(search);
  const current = useTimeStore.getState();
  if (time.window && !sameTimeWindow(current.window, time.window)) {
    current.setWindow(time.window);
  }
  if (time.anchor && current.anchor?.at !== time.anchor.at) {
    current.setAnchor(time.anchor);
  } else if (time.anchor === null && current.anchor) {
    current.clearAnchor();
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
      const filters = decodeFilters(f);
      if (encodeFilters(filters) !== encodeFilters(useFiltersStore.getState().filters)) {
        useFiltersStore.getState().setAll(filters);
      }
    } catch {
      // ignore malformed filters — keep current state
    }
  } else if (search.has('filters') && useFiltersStore.getState().filters.length > 0) {
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

export function encodeStack(frames: FrameContent[]): string {
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

/**
 * The frames an address names. They carry no ids: the stack gives a frame its
 * identity when it opens one, and a frame it already has open keeps its own.
 */
export function decodeStack(payload: string): FrameContent[] {
  const bytes = base64UrlDecode(payload);
  const json = new TextDecoder().decode(pako.inflate(bytes));
  const arr = JSON.parse(json) as Array<{
    k: FrameContent['kind'];
    p: Record<string, unknown>;
    t?: TimeWindow;
    a?: string;
    pp?: string;
    pin?: boolean;
  }>;
  return arr.map((x) => ({
    kind: x.k,
    params: x.p,
    ...(x.t !== undefined && { time_range_override: x.t }),
    ...(x.a !== undefined && { anchor_override: x.a }),
    ...(x.pp !== undefined && { parent_frame_id: x.pp }),
    pinned: x.pin ?? false,
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
