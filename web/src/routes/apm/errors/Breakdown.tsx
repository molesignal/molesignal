import { ChartBarBig } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { ErrorBreakdown, ErrorShare } from '@/api/apm';

import { formatCount } from '../format';
import { ApmCard } from '../frame/Page';
import { OTHER_DIMENSION } from '../scope';
import { languageName } from '../service/serviceFacts';

const PARTS = ['versions', 'environments', 'languages'] as const;

/**
 * Where errors happened: their occurrences by version, environment and the
 * language of the service, each a column of bars with the count after the
 * bar, as the latency distribution is drawn.
 */
export function ErrorBreakdownCard({ breakdown, description, testId }: { breakdown: ErrorBreakdown; description?: string; testId: string }) {
  const { t } = useTranslation('apm');

  const label = (part: (typeof PARTS)[number], value: string): string => {
    if (!value) return t('errors.breakdown.unknown');
    if (value === OTHER_DIMENSION) return t('overflow.other');
    return part === 'languages' ? languageName(value) : value;
  };

  return (
    <ApmCard icon={ChartBarBig} tone="text-[var(--red-ink)]" title={t('errors.breakdown.title')} description={description} testId={testId}>
      <div className="grid gap-x-6 gap-y-3 px-3 py-2 md:grid-cols-3">
        {PARTS.map((part) => (
          <section key={part} aria-label={t(`errors.breakdown.${part}`)} data-apm-error-breakdown={part} className="min-w-0">
            <h3 className="type-micro mb-1 font-sans font-strong text-tx-2">{t(`errors.breakdown.${part}`)}</h3>
            <Shares shares={breakdown[part]} label={(value) => label(part, value)} />
          </section>
        ))}
      </div>
    </ApmCard>
  );
}

function Shares({ shares, label }: { shares: readonly ErrorShare[]; label: (value: string) => string }) {
  const { t } = useTranslation('apm');
  const widest = Math.max(1, ...shares.map((share) => share.count));
  const total = shares.reduce((sum, share) => sum + share.count, 0);

  if (shares.length === 0) return <p className="type-micro font-sans text-tx-3">{t('errors.breakdown.none')}</p>;

  return (
    <ol className="space-y-0.5">
      {shares.map((share) => {
        const name = label(share.value);
        const percent = total > 0 ? Math.round((share.count / total) * 100) : 0;
        return (
          <li
            key={share.value}
            data-apm-error-share={share.value}
            title={t('errors.breakdown.row', { value: name, count: formatCount(share.count), share: `${percent}%` })}
            className="type-micro grid h-[18px] grid-cols-[minmax(0,88px)_minmax(0,1fr)] items-center gap-2 font-sans"
          >
            <span className="truncate text-right text-tx-1">{name}</span>
            <span className="relative mr-[64px] h-[10px]">
              <span className="absolute inset-y-0 left-0 rounded-[3px] bg-[var(--red)]" style={{ width: `${Math.max(0.6, (share.count / widest) * 100)}%` }} />
              <span className="pointer-events-none absolute top-1/2 flex -translate-y-1/2 items-center gap-1.5 whitespace-nowrap pl-1.5 tabular-nums" style={{ left: `${(share.count / widest) * 100}%` }}>
                <span className="text-tx-1">{formatCount(share.count)}</span>
                <span className="text-tx-3">{percent}%</span>
              </span>
            </span>
          </li>
        );
      })}
    </ol>
  );
}
