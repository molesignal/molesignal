import { Crosshair } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { RumAggregate } from '@/api/rum';

import { regionName } from '../../_helpers';
import { BarRows } from '../../overview/BarRows';
import { CardState, OverviewCard } from '../../overview/OverviewCard';
import { readSpread, type WhereKey, WHERE } from '../model';

type Read = { isPending: boolean; isError: boolean; error: unknown; refetch: () => unknown; data: RumAggregate | undefined };

/** The values of a field shown at most. */
const SHOWN = 3;

/** Where one issue happens: the sessions its errors met, by browser, by system and by country. */
export function WhereCard({ reads, language }: { reads: Record<WhereKey, Read>; language: string }) {
  const { t } = useTranslation('rum');

  return (
    <OverviewCard
      icon={Crosshair}
      tone="text-[var(--red-ink)]"
      title={t('errors.drawer.where.title')}
      description={t('errors.drawer.where.description')}
      testId="issue-where"
    >
      <div className="grid [grid-template-columns:repeat(auto-fit,minmax(min(220px,100%),1fr))]">
        {WHERE.map(({ key }) => {
          const read = reads[key];
          const spread = readSpread(read.data);
          const total = spread.reduce((sum, entry) => sum + entry.sessions, 0);

          return (
            <section key={key} data-rum-where={key} className="min-w-0">
              <h3 className="m-0 px-3 pt-2 font-sans text-xs font-strong text-tx-3">{t(`errors.drawer.where.${key}`)}</h3>
              <CardState query={read} empty={spread.length === 0} emptyTitle={t('errors.spread.empty')}>
                <BarRows
                  testId={`issue-${key}`}
                  total={total}
                  language={language}
                  barClassName="bg-[var(--red)] opacity-80"
                  rows={spread.slice(0, SHOWN).map((entry) => ({
                    key: entry.value,
                    label: key === 'country' ? regionName(entry.value, language) : entry.value,
                    count: entry.sessions,
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
