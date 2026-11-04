import { useQuery } from '@tanstack/react-query';
import type { useTranslation } from 'react-i18next';

import * as incidentsApi from '@/api/incidents';
import type { PersonLoad } from '@/api/incidents';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shell/ui/table';
import { useUsers } from '@/shell/useUsers';

import { formatDuration } from './format';
import { SectionHeading } from './SectionHeading';

type TFn = ReturnType<typeof useTranslation<'alerts'>>['t'];

/** The shortest window the load is asked for: a night has to fit in it. */
const MIN_WINDOW_SECS = 86_400;

/** Everyone's on-call load over the window: shifts, pages, and pages at night and at weekends. */
export function OncallLoadSection({ windowSecs, t }: { windowSecs: number; t: TFn }) {
  const window = Math.max(windowSecs, MIN_WINDOW_SECS);
  const load = useQuery({
    queryKey: ['alerts-oncall-load', window],
    queryFn: () => incidentsApi.oncallLoad(window),
    staleTime: 30_000,
    retry: false,
  });

  return (
    <section
      data-insights-oncall-load
      aria-labelledby="insights-oncall-load"
      className="space-y-3 rounded-md bg-[var(--functional-surface)] p-4 [box-shadow:var(--shadow-functional-surface)]"
    >
      <SectionHeading
        id="insights-oncall-load"
        title={t('insights.load.title')}
        description={t('insights.load.description', {
          start: load.data?.nightStartHour ?? 22,
          end: load.data?.nightEndHour ?? 8,
        })}
      />
      {load.isLoading ? (
        <p className="font-sans text-sm text-tx-3">{t('insights.load.loading')}</p>
      ) : load.isError ? (
        <p className="font-sans text-sm text-tx-3">{t('insights.load.error')}</p>
      ) : (
        <OncallLoadTable people={load.data?.people ?? []} t={t} />
      )}
    </section>
  );
}

/** One row per person: how long on call, how often paged, and how many of those pages came at night. */
export function OncallLoadTable({ people, t }: { people: PersonLoad[]; t: TFn }) {
  const { nameOf } = useUsers();

  if (people.length === 0) {
    return <p className="font-sans text-sm text-tx-3">{t('insights.load.empty')}</p>;
  }

  return (
    <Table data-oncall-load className="[&_tr>*:first-child]:pl-0 [&_tr>*:last-child]:pr-0">
      <TableHeader>
        <TableRow>
          <TableHead>{t('insights.load.columns.person')}</TableHead>
          <TableHead className="text-right">{t('insights.load.columns.oncall')}</TableHead>
          <TableHead className="text-right">{t('insights.load.columns.pages')}</TableHead>
          <TableHead className="text-right">{t('insights.load.columns.night')}</TableHead>
          <TableHead className="text-right">{t('insights.load.columns.weekend')}</TableHead>
          <TableHead className="text-right">{t('insights.load.columns.nights')}</TableHead>
          <TableHead className="text-right">{t('insights.load.columns.mtta')}</TableHead>
          <TableHead>{t('insights.load.columns.timezone')}</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {people.map((person) => (
          <TableRow key={person.userId} data-load-row={person.userId}>
            <TableCell className="max-w-[240px] truncate font-strong text-tx-1">{nameOf(person.userId)}</TableCell>
            <TableCell className="text-right tabular-nums">
              {person.oncallSecs > 0 ? formatDuration(person.oncallSecs, t) : '—'}
            </TableCell>
            <TableCell className="text-right tabular-nums">{person.pages}</TableCell>
            <TableCell className={person.nightPages > 0 ? 'text-right tabular-nums text-orange' : 'text-right tabular-nums'}>
              {person.nightPages}
            </TableCell>
            <TableCell className="text-right tabular-nums">{person.weekendPages}</TableCell>
            <TableCell className="text-right tabular-nums">{person.interruptedNights}</TableCell>
            <TableCell className="text-right tabular-nums">
              {person.responded > 0
                ? t('insights.load.mtta_of', {
                    mtta: formatDuration(person.mttaSecs, t),
                    count: person.responded,
                  })
                : '—'}
            </TableCell>
            <TableCell className="whitespace-nowrap text-tx-2">{person.timezone}</TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}
