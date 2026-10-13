import { type ReactNode, useState } from 'react';

import { statusPageHistoryDays } from '../model';
import { type PublicStatusContext, PublicStatusFrame } from './Frame';
import { archiveMonthWindow } from './history';
import { PublicIncidentHistory } from './IncidentHistory';
import { ArchiveMonthNavigator } from './MonthNavigator';
import type { PublicStatusSource } from './routing';
import { PublicUptimeHistory } from './UptimeHistory';

type ArchiveView = 'history' | 'uptime';

/** Everything that happened on a public status page and is over. */
export function PublicStatusHistoryPage({
  source = 'slug',
  unmatchedDomain,
}: {
  source?: PublicStatusSource;
  unmatchedDomain?: ReactNode;
}) {
  return (
    <PublicStatusFrame source={source} view="history" unmatchedDomain={unmatchedDomain}>
      {(context) => <Archive view="history" context={context} />}
    </PublicStatusFrame>
  );
}

/** How the components of a public status page have held up, day by day. */
export function PublicStatusUptimePage({
  source = 'slug',
  unmatchedDomain,
}: {
  source?: PublicStatusSource;
  unmatchedDomain?: ReactNode;
}) {
  return (
    <PublicStatusFrame source={source} view="uptime" unmatchedDomain={unmatchedDomain}>
      {(context) => <Archive view="uptime" context={context} />}
    </PublicStatusFrame>
  );
}

/**
 * The two views of the past. Both are read three months at a time, and the
 * months being read are chosen in the heading of the view, beside what it is.
 */
function Archive({ view, context }: { view: ArchiveView; context: PublicStatusContext }) {
  const { snapshot, page, language, copy } = context;
  const [endMonth, setEndMonth] = useState<number | null>(null);
  const historyDays = statusPageHistoryDays(page);
  const months = archiveMonthWindow(snapshot.generated_at, historyDays, page.timezone, endMonth);
  const headingId = `${view}-heading`;

  return (
    <section aria-labelledby={headingId}>
      <header className="flex flex-col gap-5 sm:flex-row sm:items-end sm:justify-between sm:gap-8">
        <div className="min-w-0">
          <h2 id={headingId} className="text-2xl font-display-strong tracking-[-0.02em] text-tx-0 sm:text-3xl">
            {view === 'history' ? copy('public.archive.history_title') : copy('public.archive.uptime_title')}
          </h2>
          <p className="mt-2 max-w-2xl text-pretty text-sm leading-6 text-tx-2">
            {view === 'history'
              ? copy('public.archive.history_description', { count: historyDays })
              : copy('public.archive.uptime_description')}
          </p>
        </div>
        <ArchiveMonthNavigator
          window={months}
          language={language}
          copy={copy}
          onPrevious={() => setEndMonth(months.endIndex - 1)}
          onNext={() => setEndMonth(months.endIndex + 1)}
        />
      </header>

      {view === 'history' ? (
        <PublicIncidentHistory context={context} monthKeys={months.keys} />
      ) : (
        <PublicUptimeHistory context={context} monthKeys={months.keys} />
      )}
    </section>
  );
}
