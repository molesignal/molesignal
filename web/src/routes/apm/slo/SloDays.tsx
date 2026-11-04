import { CalendarCheck } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { ServiceLevelObjective, SloDay } from '@/api/apmSlo';
import { DATE_PATTERNS, useTimeFormatter, zonedMoment } from '@/lib/time';

import { formatSli } from './model';
import { ApmCard } from '../frame/Page';

const DAY_MICROS = 86_400_000_000;

type DayState = 'met' | 'missed' | 'no_data';

const DAY_FILL: Record<DayState, string> = {
  met: 'bg-[var(--green)]',
  missed: 'bg-[var(--red)]',
  no_data: 'bg-bg-3',
};

/**
 * Every day of an objective's window as a cell, in the colour of whether it
 * met the objective, with what it met on hover. Days are whole UTC days, as
 * the backend counts them; the last one is today, still going.
 */
export function SloDays({ slo, history, now }: { slo: ServiceLevelObjective; history: readonly SloDay[]; now: number }) {
  const { t } = useTranslation('apm');
  const { dateFormat } = useTimeFormatter();
  const byDay = new Map(history.map((day) => [Math.floor(day.dayStart / DAY_MICROS), day]));
  const today = Math.floor(now / DAY_MICROS);
  const days = Array.from({ length: slo.windowDays }, (_, index) => {
    const day = today - slo.windowDays + 1 + index;
    const sli = byDay.get(day)?.sli ?? null;
    const state: DayState = sli === null ? 'no_data' : sli >= slo.objective ? 'met' : 'missed';
    return { start: day * DAY_MICROS, sli, state };
  });
  const date = (micros: number) => zonedMoment(micros / 1_000, 'UTC').format(DATE_PATTERNS[dateFormat]);
  const counted = days.filter((day) => day.state !== 'no_data').length;
  const met = days.filter((day) => day.state === 'met').length;

  return (
    <ApmCard icon={CalendarCheck} tone="text-[var(--green-ink)]" title={t('slo.history.title')} description={t('slo.history.summary', { met, count: counted })} testId="slo-history" bodyClassName="px-3 py-3">
      <ol aria-label={t('slo.history.title')} className="grid h-8 gap-0.5" style={{ gridTemplateColumns: `repeat(${days.length}, minmax(2px, 1fr))` }}>
        {days.map((day) => {
          const label = t(`slo.history.day.${day.state}`, { date: date(day.start), sli: formatSli(day.sli) });
          return <li key={day.start} data-apm-slo-day={day.state} title={label} aria-label={label} className={`rounded-sm ${DAY_FILL[day.state]}`} />;
        })}
      </ol>
      <div className="mt-1.5 flex justify-between font-sans text-xs text-tx-3">
        <span>{date(days[0]?.start ?? 0)}</span>
        <span>{date(days.at(-1)?.start ?? 0)}</span>
      </div>
    </ApmCard>
  );
}
