import type { GlobalFilter } from '@/stores/useFiltersStore';
import {
  formatWindowSummary,
  resolveWindow,
  type TimeWindow,
} from '@/stores/useTimeStore';

export const AGENT_TIME_PRESETS = {
  '15m': 15 * 60,
  '30m': 30 * 60,
  '1h': 3_600,
  '6h': 6 * 3_600,
  '24h': 24 * 3_600,
  '7d': 7 * 86_400,
} as const;

export interface InvestigationChatContext {
  environment: string;
  service: string;
  alert: string;
}

export const AGENT_CONTEXT_FILTER_KEYS = {
  environment: ['environment', 'env', 'deployment.environment'],
  service: ['service', 'service_name', 'service.name'],
  alert: ['alert', 'alert_id', 'incident_id'],
} as const;

export function chatContextFromFilters(
  filters: GlobalFilter[],
): InvestigationChatContext {
  const inclusive = filters.filter((filter) => filter.operator !== '!=');
  const valueFor = (aliases: readonly string[]) =>
    inclusive.find((filter) => aliases.includes(filter.key))?.value ?? '';
  return {
    environment: valueFor(AGENT_CONTEXT_FILTER_KEYS.environment),
    service: valueFor(AGENT_CONTEXT_FILTER_KEYS.service),
    alert: valueFor(AGENT_CONTEXT_FILTER_KEYS.alert),
  };
}

export function contextStreamHints(
  context: InvestigationChatContext,
  filters: GlobalFilter[],
): string[] {
  const hints = new Set<string>();
  if (context.environment) hints.add(`environment:${context.environment}`);
  if (context.service) hints.add(`service:${context.service}`);
  if (context.alert) hints.add(`alert:${context.alert}`);
  for (const filter of filters) {
    hints.add(
      filter.operator === '!='
        ? `${filter.key}!=${filter.value}`
        : `${filter.key}:${filter.value}`,
    );
  }
  return [...hints];
}

export function timeRangeMicros(window: TimeWindow): {
  start_micros: number;
  end_micros: number;
} {
  const resolved = resolveWindow(window);
  return {
    start_micros: resolved.from.getTime() * 1_000,
    end_micros: resolved.to.getTime() * 1_000,
  };
}

export function timePresetFromWindow(window: TimeWindow): string {
  if (window.mode !== 'relative' || window.to !== 'now') return 'custom';
  const preset = window.from.replace(/^now-/, '');
  return preset in AGENT_TIME_PRESETS ? preset : 'custom';
}

export function timeWindowForPreset(preset: string): TimeWindow | null {
  if (!(preset in AGENT_TIME_PRESETS)) return null;
  return { from: `now-${preset}`, to: 'now', mode: 'relative' };
}

export function investigationContextSummary(
  window: TimeWindow,
  filters: GlobalFilter[],
): string {
  const context = chatContextFromFilters(filters);
  return [
    context.service && `service:${context.service}`,
    context.environment && `environment:${context.environment}`,
    context.alert && `alert:${context.alert}`,
    formatWindowSummary(window),
  ]
    .filter(Boolean)
    .join(' · ');
}
