import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import * as healthApi from '@/api/health';
import { cn } from '@/shell/lib/cn';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

export type SystemStatus = 'healthy' | 'degraded' | 'disconnected';

export function resolveSystemStatus(
  health: healthApi.SystemHealth | undefined,
  error: unknown,
): SystemStatus {
  if (error instanceof healthApi.DegradedSystemHealthError) return 'degraded';
  if (error !== null && error !== undefined) return 'disconnected';
  if (health === undefined) return 'disconnected';
  return health.status === 'degraded' ? 'degraded' : 'healthy';
}

const SUCCESS_INTERVAL_MS = 30_000;
const SUCCESS_JITTER_MS = 5_000;
const FAILURE_BACKOFF_MS = [5_000, 10_000, 20_000, 30_000, 60_000] as const;

/** Select the next probe delay; a successful probe resets failureCount to 0. */
export function nextSystemHealthCheckDelay(
  failureCount: number,
  random: () => number = Math.random,
): number {
  if (failureCount > 0) {
    const backoffIndex = Math.min(
      failureCount - 1,
      FAILURE_BACKOFF_MS.length - 1,
    );
    return FAILURE_BACKOFF_MS[backoffIndex] ?? 60_000;
  }
  const sample = Math.max(0, Math.min(1, random()));
  return Math.round(
    SUCCESS_INTERVAL_MS - SUCCESS_JITTER_MS + sample * SUCCESS_JITTER_MS * 2,
  );
}

const STATUS_TONE: Record<SystemStatus, string> = {
  healthy: 'bg-green',
  degraded: 'bg-yellow',
  disconnected: 'bg-red',
};

export function SystemStatusIndicator() {
  const { t } = useTranslation('shell');
  const healthQuery = useQuery({
    queryKey: ['system', 'health'],
    queryFn: healthApi.get,
    refetchInterval: (query) =>
      nextSystemHealthCheckDelay(query.state.fetchFailureCount),
    refetchIntervalInBackground: true,
    refetchOnReconnect: false,
    refetchOnWindowFocus: false,
    retry: false,
    staleTime: 25_000,
  });
  const status = resolveSystemStatus(healthQuery.data, healthQuery.error);
  const label = t(`system_status.${status}`);

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          role="status"
          aria-label={label}
          data-state={status}
          data-testid="system-status-indicator"
          className="flex h-8 w-8 items-center justify-center rounded-md hover:bg-bg-3"
        >
          <span
            aria-hidden
            className={cn('h-2 w-2 rounded-full', STATUS_TONE[status])}
          />
        </span>
      </TooltipTrigger>
      <TooltipContent side="bottom">{label}</TooltipContent>
    </Tooltip>
  );
}
