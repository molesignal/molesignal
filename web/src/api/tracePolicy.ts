import { http } from '@/lib/http';

/** What a sampling rule does with the traces it matches. */
export type SamplingRuleAction = { type: 'keep' } | { type: 'drop' } | { type: 'ratio'; ratio: number };

/** A rule tried before the normal ratio; every field it names must match. */
export interface SamplingRule {
  name: string;
  serviceName?: string | null;
  route?: string | null;
  operation?: string | null;
  minimumDurationMs?: number | null;
  requireError: boolean;
  action: SamplingRuleAction;
}

/** From what duration a span of each kind makes its trace slow, which keeps it. */
export interface SlowThresholds {
  http: number;
  query: number;
  batchIntake: number;
  database: number;
  objectStore: number;
  external: number;
  background: number;
  other: number;
}

/** The platform's tail sampling policy: which traces are kept once they end. */
export interface TraceRuntimePolicy {
  version: number;
  enabled: boolean;
  /** The share of traces kept that no rule, error or slowness keeps. */
  normalSampleRatio: number;
  decisionWindowMs: number;
  rootGraceMs: number;
  decisionCacheMs: number;
  maxTraces: number;
  memoryBytes: number;
  maxSpansPerTrace: number;
  slowThresholdsMs: SlowThresholds;
  routeSlowThresholdsMs: Record<string, number>;
  rules: SamplingRule[];
}

export interface TraceTelemetry {
  deploymentForceDisabled: boolean;
  runtimeEnabled: boolean;
  effectiveEnabled: boolean;
  policy: TraceRuntimePolicy;
  health: { status: 'ok' | 'degraded' | 'disabled'; detail: string };
}

export const tracePolicyApi = {
  /** The policy in force and how the sampler does (`sys.telemetry.read`). */
  get: async () => {
    const { data } = await http.get<TraceTelemetry>('/system/telemetry');
    return data;
  },
  /** Publishes a new version of the policy (`sys.telemetry.manage`). */
  save: async (policy: TraceRuntimePolicy) => {
    const { data } = await http.put<TraceTelemetry>('/system/telemetry', policy);
    return data;
  },
};

export const tracePolicyQueryKeys = {
  telemetry: () => ['system', 'telemetry'] as const,
};
