import { http } from '@/lib/http';

export interface SystemHealth {
  status: 'ok' | 'degraded';
  reason?: string;
}

/**
 * A 503 still carries useful degraded-state details for the indicator, while
 * remaining a failed probe so the polling scheduler can apply backoff.
 */
export class DegradedSystemHealthError extends Error {
  readonly health: SystemHealth;

  constructor(health: SystemHealth) {
    super(health.reason ?? 'system health is degraded');
    this.name = 'DegradedSystemHealthError';
    this.health = health;
  }
}

export async function get(): Promise<SystemHealth> {
  const { data, status } = await http.get<SystemHealth>('/healthz', {
    validateStatus: (status) => status === 200 || status === 503,
  });
  if (status === 503) throw new DegradedSystemHealthError(data);
  return data;
}
