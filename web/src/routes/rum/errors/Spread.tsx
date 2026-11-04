import { Crosshair } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { RumAggregate } from '@/api/rum';

import { DIMENSIONS, type DimensionKey, type Picked, readSpread } from './model';
import { BarRows } from '../overview/BarRows';
import { CardState, OverviewCard } from '../overview/OverviewCard';

type Read = { isPending: boolean; isError: boolean; error: unknown; refetch: () => unknown; data: RumAggregate | undefined };

/** The values of a dimension shown at most. */
const SHOWN = 5;

/**
 * Where the issues listed happen: the sessions their errors met, spread over the versions, the
 * pages and the browsers. A value can be picked, which narrows the list to the errors that
 * happened there and every column to what is left; a picked value is picked again to let go.
 */
export function SpreadCard({
  reads,
  picked,
  onPick,
  language,
}: {
  reads: Record<DimensionKey, Read>;
  picked: Picked;
  onPick: (key: DimensionKey, value: string | null) => void;
  language: string;
}) {
  const { t } = useTranslation('rum');

  return (
    <OverviewCard
      icon={Crosshair}
      tone="text-[var(--red-ink)]"
      title={t('errors.spread.title')}
      description={t('errors.spread.description')}
      testId="errors-spread"
    >
      <div className="grid [grid-template-columns:repeat(auto-fit,minmax(min(280px,100%),1fr))] divide-bd-0 md:divide-x">
        {DIMENSIONS.map(({ key }) => {
          const read = reads[key];
          const spread = readSpread(read.data);
          const total = spread.reduce((sum, entry) => sum + entry.sessions, 0);
          const shown = spread.slice(0, SHOWN);
          const chosen = picked[key];

          // What is picked stays in view, whatever the other columns leave of it, to be let go of.
          if (chosen && !shown.some((entry) => entry.value === chosen)) {
            shown.push(spread.find((entry) => entry.value === chosen) ?? { value: chosen, sessions: 0 });
          }

          return (
            <section key={key} data-rum-spread={key} className="min-w-0">
              <h3 className="m-0 px-3 pt-2 font-sans text-xs font-strong text-tx-3">{t(`errors.spread.${key}`)}</h3>
              <CardState query={read} empty={shown.length === 0} emptyTitle={t('errors.spread.empty')}>
                <BarRows
                  testId={`errors-${key}`}
                  total={total}
                  language={language}
                  barClassName="bg-[var(--red)] opacity-80"
                  rows={shown.map((entry) => ({
                    key: entry.value,
                    label: entry.value,
                    count: entry.sessions,
                    active: chosen === entry.value,
                    onSelect: () => onPick(key, chosen === entry.value ? null : entry.value),
                  }))}
                />
              </CardState>
            </section>
          );
        })}
      </div>
    </OverviewCard>
  );
}
