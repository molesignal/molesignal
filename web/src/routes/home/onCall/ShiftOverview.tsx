import { useTranslation } from 'react-i18next';

import { uiLabelClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';

import type { OnCallShiftOverview } from './model';

interface ShiftMetricProps {
  label: string;
  value: number | string;
  warning?: boolean;
}

function ShiftMetric({ label, value, warning = false }: ShiftMetricProps) {
  return (
    <div className="min-w-0">
      <div className={uiLabelClass}>{label}</div>
      <div
        className={cn(
          'mt-0.5 font-sans text-base font-display-strong tabular-nums text-tx-0',
          warning && 'text-orange-soft',
        )}
      >
        {value}
      </div>
    </div>
  );
}

export interface OnCallShiftOverviewProps {
  overview: OnCallShiftOverview | null;
  scheduleName: string;
  startedAt: string | null;
  elapsed: string | null;
  onViewSchedule: () => void;
  onViewEscalations: () => void;
}

export function OnCallShiftOverviewPanel({
  overview,
  scheduleName,
  startedAt,
  elapsed,
  onViewSchedule,
  onViewEscalations,
}: OnCallShiftOverviewProps) {
  const { t } = useTranslation('onboarding');
  const policyNames = overview?.escalationPolicyNames ?? [];
  const policyName =
    overview === null
      ? '—'
      : policyNames.length === 0
        ? t('home.on_call.no_escalation_policy')
        : policyNames.length === 1
          ? policyNames[0]
          : t('home.on_call.more_escalation_policies', {
              name: policyNames[0],
              count: policyNames.length - 1,
            });
  const pendingCount = overview?.pendingCount;

  return (
    <section
      aria-label={t('home.on_call.overview_title')}
      className="mt-2"
      data-testid="on-call-shift-overview"
    >
      <div className="flex min-w-0 items-baseline justify-between gap-3">
        <div className={uiLabelClass}>
          {t('home.on_call.overview_title')}
        </div>
        <div className="min-w-0 truncate text-right font-sans text-xs text-tx-2">
          <span>{t('home.on_call.elapsed_label')} </span>
          <span className="font-strong tabular-nums text-tx-0">
            {elapsed ?? '—'}
          </span>
        </div>
      </div>

      <div className="mt-1.5 grid grid-cols-2 gap-x-4 gap-y-2 border-y border-bd-0 py-1.5 min-[430px]:grid-cols-4">
        <ShiftMetric
          label={t('home.on_call.shift_incidents')}
          value={overview?.incidentCount ?? '—'}
        />
        <ShiftMetric
          label={t('home.on_call.pending_label')}
          value={pendingCount ?? '—'}
          warning={pendingCount != null && pendingCount > 0}
        />
        <ShiftMetric
          label={t('home.on_call.acknowledged_label')}
          value={overview?.acknowledgedCount ?? '—'}
        />
        <ShiftMetric
          label={t('home.on_call.escalated_label')}
          value={overview?.escalatedCount ?? '—'}
        />
      </div>

      <div className="flex min-h-11 min-w-0 flex-wrap items-center gap-x-2 font-sans text-xs text-tx-2 sm:min-h-7">
        <button
          type="button"
          onClick={onViewSchedule}
          className="inline-flex h-11 max-w-full items-center truncate font-strong text-tx-1 hover:text-tx-0 focus-visible:outline-none focus-visible:underline focus-visible:underline-offset-4 sm:h-7"
          title={scheduleName}
        >
          {scheduleName}
        </button>
        <span aria-hidden="true" className="text-tx-3">
          ·
        </span>
        <button
          type="button"
          onClick={onViewEscalations}
          className="inline-flex h-11 max-w-full items-center truncate font-strong text-tx-1 hover:text-tx-0 focus-visible:outline-none focus-visible:underline focus-visible:underline-offset-4 sm:h-7"
          title={policyNames.join(' · ') || policyName}
        >
          {policyName}
        </button>
        {startedAt && (
          <>
            <span aria-hidden="true" className="text-tx-3">
              ·
            </span>
            <span className="truncate tabular-nums">
              {t('home.on_call.started_at', { time: startedAt })}
            </span>
          </>
        )}
      </div>
    </section>
  );
}
