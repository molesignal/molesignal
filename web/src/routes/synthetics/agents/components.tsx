import { useTranslation } from 'react-i18next';

import type { ProbeAgent } from '@/api/synthetics';
import { Dot, Pill, type PillTone } from '@/shell/chrome';

import type { AgentStatus } from './model';

const STATUS_TONE: Record<AgentStatus, PillTone> = {
  registered: 'indigo',
  online: 'green',
  degraded: 'yellow',
  offline: 'red',
  draining: 'blue',
  revoked: 'dim',
};

const STATUS_DOT: Record<AgentStatus, 'indigo' | 'green' | 'yellow' | 'red' | 'blue' | 'dim'> = {
  registered: 'indigo',
  online: 'green',
  degraded: 'yellow',
  offline: 'red',
  draining: 'blue',
  revoked: 'dim',
};

export function AgentStatusPill({ status }: { status: AgentStatus }) {
  const { t } = useTranslation('synthetics');
  return (
    <Pill tone={STATUS_TONE[status]}>
      <Dot tone={STATUS_DOT[status]} />
      {t(`states.${status}`)}
    </Pill>
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
