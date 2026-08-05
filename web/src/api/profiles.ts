import { http, toApiError } from '@/lib/http';

/**
 * Continuous Profiling client. Talks to the backend profiles routes
 * (crates/api/src/http/routes/profiles.rs):
 *
 *  - `GET /profiles`            metadata list / filter
 *  - `GET /profiles/flamegraph` window-merged flamebearer (+ trace correlation)
 *  - `POST /profiles/flamegraph/selection` exact merge for selected profiles
 *  - `GET /profiles/diff`       baseline vs comparison diff flamebearer
 *  - `GET /profiles/{id}`       raw pprof download
 *
 * Flamebearer payloads are camelCase (`numTicks`, `maxSelf`, …) because the
 * Rust structs use `serde(rename_all = "camelCase")`; the response envelopes
 * keep snake_case (`profile_count`, `baseline_count`, …).
 */

export type ProfileTypeName =
  | 'cpu'
  | 'wall'
  | 'alloc_space'
  | 'alloc_objects'
  | 'inuse_space'
  | 'inuse_objects'
  | 'goroutines'
  | 'lock'
  | 'samples'
  | (string & {});

export interface ProfileEntry {
  id: string;
  service: string;
  profile_type: ProfileTypeName;
  timestamp: number;
  total_value: number;
  sample_count: number;
  duration_nanos: number;
  unsymbolized: boolean;
  trace_id?: string;
  span_id?: string;
}

/** Single-window flamebearer (`names[]` + per-depth flat `levels[]`).
 *  Each single bar is 4 ints: `[offset, total, self, nameIndex]`. */
export interface Flamebearer {
  names: string[];
  levels: number[][];
  numTicks: number;
  maxSelf: number;
  units: string;
}

/** Diff flamebearer. Each bar is 5 ints: `[offset, total, self, nameIndex, delta]`
 *  where `total` is both windows summed and `delta` is comparison − baseline. */
export interface DiffFlamebearer {
  names: string[];
  levels: number[][];
  numTicks: number;
  maxSelf: number;
  maxAbsDelta: number;
  units: string;
}

export interface FlamegraphResult {
  flamebearer: Flamebearer;
  truncated: boolean;
  profile_count: number;
}

export interface DiffResult {
  flamebearer: DiffFlamebearer;
  truncated: boolean;
  baseline_count: number;
  comparison_count: number;
}

export interface ListParams {
  service?: string | undefined;
  type?: ProfileTypeName | undefined;
  /** Window bounds in microseconds. */
  from?: number | undefined;
  to?: number | undefined;
  /** `key:value` single-label filter. */
  label?: string | undefined;
  trace_id?: string | undefined;
  limit?: number | undefined;
}

export interface FlamegraphParams {
  service?: string | undefined;
  type?: ProfileTypeName | undefined;
  from?: number | undefined;
  to?: number | undefined;
  label?: string | undefined;
  trace_id?: string | undefined;
  span_id?: string | undefined;
  max_merge?: number | undefined;
}

export interface FlamegraphSelectionParams {
  profile_ids: string[];
  max_merge?: number | undefined;
}

export interface DiffParams {
  service?: string | undefined;
  type?: ProfileTypeName | undefined;
  /** Comparison window (micros). */
  from?: number | undefined;
  to?: number | undefined;
  /** Baseline window (micros). */
  baseline_from?: number | undefined;
  baseline_to?: number | undefined;
  label?: string | undefined;
  max_merge?: number | undefined;
}

function queryParams(params: Record<string, string | number | undefined>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [key, value] of Object.entries(params)) {
    if (value === undefined || value === '' || value === null) continue;
    out[key] = String(value);
  }
  return out;
}

/** The profiles stream is created on first ingest (schema-on-write); before
 *  that the query planner returns "stream not found". Empty instances should
 *  render the onboarding empty state, not an error — so callers treat that one
 *  message as "no data yet". */
export function isStreamNotFound(err: unknown): boolean {
  return toApiError(err).message.includes('stream not found');
}

const EMPTY_FLAMEBEARER: Flamebearer = {
  names: ['total'],
  levels: [],
  numTicks: 0,
  maxSelf: 0,
  units: '',
};

export async function list(params: ListParams = {}): Promise<ProfileEntry[]> {
  try {
    const { data } = await http.get<{ profiles: ProfileEntry[] }>('/profiles', {
      params: queryParams({ ...params }),
    });
    return data.profiles ?? [];
  } catch (err) {
    if (isStreamNotFound(err)) return [];
    throw err;
  }
}

export async function flamegraph(params: FlamegraphParams = {}): Promise<FlamegraphResult> {
  try {
    const { data } = await http.get<FlamegraphResult>('/profiles/flamegraph', {
      params: queryParams({ ...params }),
    });
    return data;
  } catch (err) {
    if (isStreamNotFound(err)) {
      return { flamebearer: EMPTY_FLAMEBEARER, truncated: false, profile_count: 0 };
    }
    throw err;
  }
}

/** Build a flame graph from the exact profile rows selected in the workbench. */
export async function flamegraphSelection(
  params: FlamegraphSelectionParams,
): Promise<FlamegraphResult> {
  const { data } = await http.post<FlamegraphResult>(
    '/profiles/flamegraph/selection',
    params,
  );
  return data;
}

export async function diff(params: DiffParams = {}): Promise<DiffResult> {
  const { data } = await http.get<DiffResult>('/profiles/diff', {
    params: queryParams({ ...params }),
  });
  return data;
}

/**
 * Uploads a raw pprof profile (gzip-compressed or plain protobuf) to the direct
 * upload endpoint. The backend sniffs gzip vs raw, so the caller just streams
 * the file bytes; `service` / `type` ride as query params.
 */
export async function upload(
  file: File | Blob,
  params: { service?: string | undefined; type?: ProfileTypeName | undefined } = {},
): Promise<void> {
  await http.post('/profiles/upload', file, {
    params: queryParams({ ...params }),
    headers: { 'Content-Type': 'application/octet-stream' },
  });
}

/**
 * Fetches the archived pprof for a profile and triggers a browser download.
 * The endpoint requires the bearer token, so we stream it through axios rather
 * than handing the browser a bare `<a href>` (which would drop auth).
 */
export async function download(id: string, filename?: string): Promise<void> {
  const { data } = await http.get<Blob>(`/profiles/${encodeURIComponent(id)}`, {
    responseType: 'blob',
  });
  const url = URL.createObjectURL(data);
  try {
    const a = document.createElement('a');
    a.href = url;
    a.download = filename ?? `${id}.pprof.gz`;
    document.body.appendChild(a);
    a.click();
    a.remove();
  } finally {
    URL.revokeObjectURL(url);
  }
}
