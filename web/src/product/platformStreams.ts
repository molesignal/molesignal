/**
 * The streams the platform writes for each organization (`PLATFORM_STREAMS`):
 * synthetic runs, the audit trail, the agent's traces and scores. A pipeline
 * may read them, never write them or change them as they arrive, and they say
 * nothing about data the organization sent.
 */
const PLATFORM_STREAMS = new Set(['_synthetic_results', '_audit', '_agent_model_traces', '_agent_score_traces']);

export function isPlatformStream(name: string): boolean {
  return PLATFORM_STREAMS.has(name);
}
