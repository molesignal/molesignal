import { useTranslation } from 'react-i18next';

import type * as agentApi from '@/api/agent';
import { cn } from '@/shell/lib/cn';

/** How a server answered when it was last tested or synchronized. */
export function McpStatusBadge({ status }: { status: agentApi.McpServer['status'] }) {
  const { t } = useTranslation('agent');
  return (
    <span
      data-agent-mcp-status={status}
      className={cn(
        'inline-flex h-5 items-center rounded px-1.5 text-type-micro font-strong',
        status === 'healthy' && 'bg-[var(--green-tint)] text-[var(--green-ink)]',
        status === 'error' && 'bg-red-dim text-[var(--red-ink)]',
        status === 'connecting' && 'bg-[var(--yellow-tint)] text-[var(--yellow-ink)]',
        !['healthy', 'error', 'connecting'].includes(status) && 'bg-bg-3 text-tx-2',
      )}
    >
      {t(`settings.tools.mcp.status.${status}`, { defaultValue: status })}
    </span>
  );
}
