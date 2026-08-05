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
  hasError: boolean,
): SystemStatus {
  if (hasError || health === undefined) return 'disconnected';
  return health.status === 'degraded' ? 'degraded' : 'healthy';
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
    refetchInterval: 15_000,
    refetchIntervalInBackground: true,
    retry: false,
    staleTime: 10_000,
  });
  const status = resolveSystemStatus(healthQuery.data, healthQuery.isError);
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
