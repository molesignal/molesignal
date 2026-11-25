/**
 * A node's configuration (schema v1, `domain::node_agent::NodeConfig`) as a form.
 *
 * Collectors are a checklist: every collector the agent ships runs unless it is unchecked, and the
 * unchecked ones are written as `collectors.deny`. Numbers and the filter left empty keep what the
 * agent has on its own.
 */

/** The collectors of the agent's capability manifest, by kind. */
export const COLLECTOR_GROUPS = {
  metrics: [
    'arp', 'ascend_npu', 'cpu_stat', 'cpu_util', 'diskio', 'iolatency', 'loadavg', 'memory_buddyinfo',
    'memory_events', 'memory_free', 'memory_others', 'memory_reclaim', 'memory_vmstat', 'metax_gpu',
    'mountpoint_perm', 'netdev', 'netdev_dcb', 'netdev_hw', 'netdev_qdisc', 'netdev_rdma_link', 'netstat',
    'runqlat', 'sockstat', 'softirq', 'tcp_memory',
  ],
  events: [
    'dropwatch', 'hungtask', 'memory_reclaim_events', 'net_rx_latency', 'netdev_bonding_lacp',
    'netdev_events', 'netdev_txqueue_timeout', 'oom', 'ras', 'softirq_tracing', 'softlockup',
    'tcp_retransmit',
  ],
  autotracing: ['cpuidle', 'cpusys', 'dload', 'iotracing', 'memburst'],
} as const;

export type CollectorGroup = keyof typeof COLLECTOR_GROUPS;

const KNOWN: readonly string[] = Object.values(COLLECTOR_GROUPS).flat();

export interface NodeConfigDocument {
  collectors?: { allow?: string[]; deny?: string[] };
  dropwatch_filter?: string;
  memory_reclaim_blocked_threshold_ns?: number;
  profiling?: {
    oncpu?: boolean;
    offcpu?: boolean;
    interval_secs?: number;
    frequency_hz?: number;
    max_workloads?: number;
    offcpu_min_duration_us?: number;
  };
}

export interface ConfigDraft {
  /** Collectors that run. */
  enabled: string[];
  /** Collectors the node reported that the manifest does not list. */
  extra: string[];
  oncpu: boolean;
  offcpu: boolean;
  intervalSecs: string;
  frequencyHz: string;
  maxWorkloads: string;
  offcpuMinDurationUs: string;
  dropwatchFilter: string;
  reclaimThresholdMs: string;
}

export type ConfigField = Exclude<keyof ConfigDraft, 'enabled' | 'extra' | 'oncpu' | 'offcpu'>;

/** Every collector the form lists: the manifest's, then any other the node reported. */
export function collectorNames(reported: readonly string[]): { known: readonly string[]; extra: string[] } {
  return { known: KNOWN, extra: reported.filter((name) => !KNOWN.includes(name)).sort() };
}

const text = (value: number | undefined) => (value === undefined ? '' : String(value));

export function draftFromConfig(config: NodeConfigDocument, reported: readonly string[]): ConfigDraft {
  const { known, extra } = collectorNames(reported);
  const all = [...known, ...extra];
  const allow = config.collectors?.allow ?? [];
  const deny = new Set(config.collectors?.deny ?? []);
  const runs = (name: string) => (allow.length === 0 || allow.includes(name)) && !deny.has(name);
  const threshold = config.memory_reclaim_blocked_threshold_ns;

  return {
    enabled: all.filter(runs),
    extra,
    oncpu: config.profiling?.oncpu ?? false,
    offcpu: config.profiling?.offcpu ?? false,
    intervalSecs: text(config.profiling?.interval_secs),
    frequencyHz: text(config.profiling?.frequency_hz),
    maxWorkloads: text(config.profiling?.max_workloads),
    offcpuMinDurationUs: text(config.profiling?.offcpu_min_duration_us),
    dropwatchFilter: config.dropwatch_filter ?? '',
    reclaimThresholdMs: threshold === undefined ? '' : String(threshold / 1_000_000),
  };
}

class Invalid {
  constructor(readonly field: ConfigField) {}
}

/** A whole number within the range, or nothing when left empty. */
function whole(draft: ConfigDraft, field: ConfigField, min: number, max: number): number | undefined {
  const raw = draft[field].trim();
  if (!raw) return undefined;

  const value = Number(raw);
  if (!Number.isInteger(value) || value < min || value > max) throw new Invalid(field);
  return value;
}

/** The document the node is sent, or the first field out of range. */
export function configFromDraft(
  draft: ConfigDraft,
): { config: NodeConfigDocument } | { field: ConfigField } {
  try {
    const config: NodeConfigDocument = {};
    const deny = [...KNOWN, ...draft.extra].filter((name) => !draft.enabled.includes(name));
    if (deny.length > 0) config.collectors = { deny };

    const profiling: NonNullable<NodeConfigDocument['profiling']> = {};
    if (draft.oncpu) profiling.oncpu = true;
    if (draft.offcpu) profiling.offcpu = true;

    const numbers = [
      ['interval_secs', whole(draft, 'intervalSecs', 1, 3600)],
      ['frequency_hz', whole(draft, 'frequencyHz', 1, 1000)],
      ['max_workloads', whole(draft, 'maxWorkloads', 1, 1024)],
      ['offcpu_min_duration_us', whole(draft, 'offcpuMinDurationUs', 0, 10_000_000)],
    ] as const;

    for (const [key, value] of numbers) {
      if (value !== undefined) profiling[key] = value;
    }

    if (Object.keys(profiling).length > 0) config.profiling = profiling;

    const filter = draft.dropwatchFilter.trim();
    if (filter) config.dropwatch_filter = filter;

    const threshold = whole(draft, 'reclaimThresholdMs', 1, 60_000);
    if (threshold !== undefined) config.memory_reclaim_blocked_threshold_ns = threshold * 1_000_000;

    return { config };
  } catch (error) {
    if (error instanceof Invalid) return { field: error.field };
    throw error;
  }
}
