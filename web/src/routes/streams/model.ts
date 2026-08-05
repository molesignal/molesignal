import type {
  FieldType,
  StreamRuntime,
  StreamRuntimeStatus,
  StreamSummary,
  StreamType,
} from '@/api/streams';
import { isQueryable } from '@/api/streams';

export type VisibleStreamType = Exclude<StreamType, 'extend'>;

export interface DisplayStreamVariant {
  id: string;
  type: VisibleStreamType;
  description: string;
  queryable: boolean;
  retentionDays: number;
  runtime: StreamRuntime | null;
}

export interface DisplayStreamRuntime {
  status: StreamRuntimeStatus;
  rows: number;
  stored_bytes: number;
  current_stored_bytes: number;
  first_received_at_micros: number | null;
  last_received_at_micros: number | null;
  stats_available: boolean;
}

export interface DisplayStream {
  key: string;
  name: string;
  types: VisibleStreamType[];
  variants: DisplayStreamVariant[];
  description: string;
  queryable: boolean;
  retentionDays: number[];
  runtime: DisplayStreamRuntime | null;
}

const STREAM_TYPE_ORDER: readonly VisibleStreamType[] = [
  'logs',
  'metrics',
  'traces',
  'profiles',
];

export type LogicalFieldType =
  | 'boolean'
  | 'integer'
  | 'decimal'
  | 'string'
  | 'timestamp'
  | 'json';

const LOGICAL_FIELD_TYPE: Record<FieldType, LogicalFieldType> = {
  bool: 'boolean',
  int64: 'integer',
  float64: 'decimal',
  utf8: 'string',
  timestamp: 'timestamp',
  json: 'json',
};

export function logicalFieldType(type: FieldType): LogicalFieldType {
  return LOGICAL_FIELD_TYPE[type];
}

/**
 * 详情页仍编辑一个具体的 typed stream，但同名 stream 的其他 signal 必须可见、可切换。
 * 当前 stream 总是被保留，避免列表请求失败或被 limit 截断时详情丢失自身类型。
 */
export function streamVariantsForDetail(
  current: StreamSummary,
  summaries: StreamSummary[],
): StreamSummary[] {
  if (current.stream_type === 'extend') return [current];

  const variants = new Map<string, StreamSummary>([[current.id, current]]);
  for (const summary of summaries) {
    if (summary.name === current.name && summary.stream_type !== 'extend') {
      variants.set(summary.id, summary);
    }
  }

  return Array.from(variants.values()).sort(
    (left, right) =>
      STREAM_TYPE_ORDER.indexOf(left.stream_type as VisibleStreamType) -
      STREAM_TYPE_ORDER.indexOf(right.stream_type as VisibleStreamType),
  );
}

function minNullable(values: Array<number | null>): number | null {
  const present = values.filter((value): value is number => value != null);
  return present.length > 0 ? Math.min(...present) : null;
}

function maxNullable(values: Array<number | null>): number | null {
  const present = values.filter((value): value is number => value != null);
  return present.length > 0 ? Math.max(...present) : null;
}

function aggregateRuntime(variants: DisplayStreamVariant[]): DisplayStreamRuntime | null {
  if (variants.every((variant) => variant.runtime == null)) return null;

  const runtimes = variants
    .map((variant) => variant.runtime)
    .filter((runtime): runtime is StreamRuntime => runtime != null);
  const latestRuntime = runtimes
    .filter((runtime) => runtime.last_received_at_micros != null)
    .reduce<StreamRuntime | null>((latest, candidate) => {
      if (latest == null) return candidate;
      return candidate.last_received_at_micros! > latest.last_received_at_micros!
        ? candidate
        : latest;
    }, null);
  const status: StreamRuntimeStatus =
    latestRuntime?.status ??
    (runtimes.some((runtime) => runtime.status === 'unknown') ? 'unknown' : 'unused');

  return {
    status,
    rows: runtimes.reduce((sum, runtime) => sum + runtime.rows, 0),
    stored_bytes: runtimes.reduce((sum, runtime) => sum + runtime.stored_bytes, 0),
    current_stored_bytes: runtimes.reduce(
      (sum, runtime) => sum + runtime.current_stored_bytes,
      0,
    ),
    first_received_at_micros: minNullable(
      runtimes.map((runtime) => runtime.first_received_at_micros),
    ),
    last_received_at_micros: maxNullable(
      runtimes.map((runtime) => runtime.last_received_at_micros),
    ),
    stats_available:
      runtimes.length === variants.length && runtimes.every((runtime) => runtime.stats_available),
  };
}

/**
 * 数据流的存储身份仍是 `(org, name, type)`；列表则以 `name` 为展示身份。
 * 因此同名、不同类型的定义在这里合成一行，具体类型的 id 和设置保留在 variants 中。
 */
export function groupStreamsByName(
  summaries: StreamSummary[],
  runtimes: StreamRuntime[],
): DisplayStream[] {
  const runtimeById = new Map(runtimes.map((runtime) => [runtime.id, runtime] as const));
  const byName = new Map<string, DisplayStreamVariant[]>();

  for (const summary of summaries) {
    if (summary.stream_type === 'extend') continue;
    const variants = byName.get(summary.name) ?? [];
    variants.push({
      id: summary.id,
      type: summary.stream_type,
      description: summary.settings.description?.trim() || '',
      queryable: isQueryable(summary),
      retentionDays: summary.effective_retention.days,
      runtime: runtimeById.get(summary.id) ?? null,
    });
    byName.set(summary.name, variants);
  }

  return Array.from(byName, ([name, unsortedVariants]) => {
    const variants = [...unsortedVariants].sort(
      (left, right) =>
        STREAM_TYPE_ORDER.indexOf(left.type) - STREAM_TYPE_ORDER.indexOf(right.type),
    );
    const types = Array.from(new Set(variants.map((variant) => variant.type)));
    const retentionDays = Array.from(
      new Set(variants.map((variant) => variant.retentionDays)),
    ).sort((left, right) => left - right);

    return {
      key: name,
      name,
      types,
      variants,
      description: variants.find((variant) => variant.description)?.description ?? '',
      queryable: variants.some((variant) => variant.queryable),
      retentionDays,
      runtime: aggregateRuntime(variants),
    };
  }).sort((left, right) => left.name.localeCompare(right.name));
}

export function selectStreamVariant(
  stream: DisplayStream,
  preferredType?: VisibleStreamType,
  queryableOnly = false,
): DisplayStreamVariant | undefined {
  const candidates = queryableOnly
    ? stream.variants.filter((variant) => variant.queryable)
    : stream.variants;
  return candidates.find((variant) => variant.type === preferredType) ?? candidates[0];
}
