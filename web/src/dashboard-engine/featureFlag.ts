export const DASHBOARD_ENGINE_FLAG = 'dashboard_engine';

const FALSE_VALUES = new Set(['0', 'false', 'off', 'disabled']);

/**
 * The dashboard engine is enabled by default and can be disabled at build time.
 */
export function isDashboardEngineEnabled(): boolean {
  const value = import.meta.env.VITE_DASHBOARD_ENGINE;
  return !FALSE_VALUES.has(String(value ?? 'true').trim().toLowerCase());
}
