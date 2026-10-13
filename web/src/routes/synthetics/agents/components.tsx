import { useTranslation } from 'react-i18next';

import type { ProbeAgent } from '@/api/synthetics';
import { cn } from '@/shell/lib/cn';

import type { AgentStatus } from './model';

const STATUS_TONE: Record<AgentStatus, { color: string; text: string }> = {
  registered: { color: 'var(--indigo)', text: 'text-indigo' },
  online: { color: 'var(--green)', text: 'text-green' },
  degraded: { color: 'var(--yellow)', text: 'text-yellow' },
  offline: { color: 'var(--red)', text: 'text-red' },
  draining: { color: 'var(--blue)', text: 'text-blue' },
  revoked: { color: 'var(--tx-3)', text: 'text-tx-2' },
};

/** Where an agent stands, in a word and its colour, as checks and locations say theirs. */
export function AgentStatusPill({ status }: { status: AgentStatus }) {
  const { t } = useTranslation('synthetics');
  const tone = STATUS_TONE[status];

  return (
    <span
      data-agent-status={status}
      className={cn('inline-flex min-w-0 items-center gap-[6px] font-sans text-xs font-strong', tone.text)}
    >
      <span
        aria-hidden="true"
        className="h-[7px] w-[7px] shrink-0 rounded-full"
        style={{ background: tone.color }}
      />
      <span className="truncate">{t(`states.${status}`)}</span>
    </span>
  );
}

export function CapacityMeter({
  label,
  available,
  maximum,
}: {
  label: string;
  available: number;
  maximum: number;
}) {
  const percent = maximum > 0 ? Math.min(100, Math.max(0, (available / maximum) * 100)) : 0;
  const tone = available === 0 && maximum > 0 ? 'bg-red' : percent < 25 ? 'bg-yellow' : 'bg-green';
  return (
    <div>
      <div className="flex items-center justify-between gap-3 text-xs">
        <span className="text-tx-2">{label}</span>
        <span className="font-code tabular-nums text-tx-1">
          {available} / {maximum}
        </span>
      </div>
      <div
        role="meter"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={maximum}
        aria-valuenow={available}
        className="mt-2 h-1.5 overflow-hidden rounded-full bg-bg-4"
      >
        <span className={`block h-full rounded-full ${tone}`} style={{ width: `${percent}%` }} />
      </div>
    </div>
  );
}

export function CompactCapacity({ capacity }: { capacity: ProbeAgent['capacity'] }) {
  const { t } = useTranslation('synthetics');
  return (
    <div className="min-w-[112px]">
      <CapacityMeter
        label={t('agents.concurrent_short')}
        available={capacity.available}
        maximum={capacity.max_concurrent}
      />
    </div>
  );
}
