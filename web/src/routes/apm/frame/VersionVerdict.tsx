import { useTranslation } from 'react-i18next';

import type { VersionStatus, VersionVerdict } from '@/api/apm';
import { cn } from '@/shell/lib/cn';

import { formatSigned } from '../format';

/** Each verdict in the ink of what it says. */
export const VERDICT_TEXT: Record<VersionStatus, string> = {
  regressed: 'text-[var(--red-ink)]',
  improved: 'text-[var(--green-ink)]',
  neutral: 'text-tx-2',
  insufficient_data: 'text-tx-3',
};

/**
 * A version's verdict against the one of its service first seen before it,
 * as a coloured word; a worse version also says what got worse and by how
 * much. The title names the version told against and both moves.
 */
export function VerdictWord({ verdict, detailed = false, className }: { verdict: VersionVerdict; detailed?: boolean; className?: string }) {
  const { t } = useTranslation('apm');
  const rate = formatSigned(verdict.delta.errorRateAbsolute, true);
  const p95 = formatSigned(verdict.delta.p95AbsoluteTime, false, true);
  const worse = verdict.status === 'regressed' ? (verdict.worse ?? []) : [];

  return (
    <span
      data-apm-verdict={verdict.status}
      title={t('deployments.verdict_title', { previous: verdict.previous, rate, p95 })}
      className={cn('flex min-w-0 items-center gap-1.5', className)}
    >
      <span className={cn('shrink-0 font-strong', VERDICT_TEXT[verdict.status])}>{t(`deployments.status.${verdict.status}`)}</span>
      {detailed && worse.length > 0 && (
        <span className="truncate tabular-nums text-tx-2">{worse.map((what) => t(`deployments.worse.${what}`, { delta: what === 'latency' ? p95 : rate })).join(' · ')}</span>
      )}
    </span>
  );
}
