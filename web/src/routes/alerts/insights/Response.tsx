import * as React from 'react';
import type { useTranslation } from 'react-i18next';

import type { BreakdownRow, ResponseInsights } from '@/api/incidents';
import { useTeams } from '@/shell/incident/Routing';
import { Readout } from '@/shell/readout/Readout';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shell/ui/table';
import { useUsers } from '@/shell/useUsers';

import { formatDuration, formatPercent } from './format';
import { SectionHeading } from './SectionHeading';

type TFn = ReturnType<typeof useTranslation<'alerts'>>['t'];
type Dimension = 'teams' | 'services' | 'people' | 'priorities';

const DIMENSIONS: Dimension[] = ['teams', 'services', 'people', 'priorities'];
const VIA_ORDER = ['web', 'im', 'sms', 'phone', 'api', 'agent'];

/**
 * How people responded over the window: how soon someone took an incident, how soon it recovered,
 * how much escalated, which step answered and how, compared with the window before; broken down by
 * team, service and person.
 */
export function ResponseSection({ response, t }: { response: ResponseInsights; t: TFn }) {
  const { current, previous } = response;
  const hasPrevious = previous.incidents > 0;

  // Shorter is better for times, lower for the escalation rate.
  const change = (now: number, before: number, lowerIsBetter: { better: string; worse: string }) => {
    if (!hasPrevious || before <= 0 || now <= 0) return t('insights.response.no_previous');
    const ratio = (now - before) / before;
    if (Math.abs(ratio) < 0.05) return t('insights.response.same');
    return t(ratio < 0 ? lowerIsBetter.better : lowerIsBetter.worse, { change: formatPercent(Math.abs(ratio)) });
  };
  const times = { better: 'insights.response.faster', worse: 'insights.response.slower' };
  const rates = { better: 'insights.response.lower', worse: 'insights.response.higher' };

  return (
    <section
      data-insights-response
      aria-labelledby="insights-response"
      className="space-y-6 rounded-md bg-[var(--functional-surface)] p-4 [box-shadow:var(--shadow-functional-surface)]"
    >
      <SectionHeading
        id="insights-response"
        title={t('insights.response.title')}
        description={t('insights.response.description')}
      />
      <div className="grid gap-x-8 gap-y-5 sm:grid-cols-2 xl:grid-cols-4">
        <Readout
          label={t('insights.response.mtta')}
          value={current.responded ? formatDuration(current.mtta.meanSecs, t) : '—'}
          detail={
            current.responded
              ? `${t('insights.response.percentiles', {
                  p50: formatDuration(current.mtta.p50Secs, t),
                  p90: formatDuration(current.mtta.p90Secs, t),
                })} · ${change(current.mtta.meanSecs, previous.mtta.meanSecs, times)}`
              : t('insights.response.none_responded')
          }
        />
        <Readout
          label={t('insights.response.mttr')}
          value={current.mttr.p50Secs > 0 ? formatDuration(current.mttr.p50Secs, t) : '—'}
          detail={
            current.mttr.p50Secs > 0
              ? `${t('insights.response.p90', { p90: formatDuration(current.mttr.p90Secs, t) })} · ${change(current.mttr.p50Secs, previous.mttr.p50Secs, times)}`
              : undefined
          }
        />
        <Readout
          label={t('insights.response.escalation_rate')}
          value={formatPercent(current.escalationRate)}
          tone={current.escalationRate > 0.3 ? 'warn' : undefined}
          detail={change(current.escalationRate, previous.escalationRate, rates)}
        />
        <Readout
          label={t('insights.response.responded')}
          value={formatPercent(current.incidents ? current.responded / current.incidents : 0)}
          detail={t('insights.response.responded_detail', {
            responded: current.responded,
            incidents: current.incidents,
          })}
        />
      </div>
      <div className="grid gap-10 xl:grid-cols-2">
        <StepRates steps={current.steps} t={t} />
        <AckVia via={current.ackVia} t={t} />
      </div>
      <Breakdown breakdown={response.breakdown} t={t} />
    </section>
  );
}

function StepRates({ steps, t }: { steps: ResponseInsights['current']['steps']; t: TFn }) {
  return (
    <div data-insights-steps>
      <SectionHeading
        id="insights-steps"
        title={t('insights.response.steps_title')}
        description={t('insights.response.steps_description')}
      />
      {steps.length === 0 ? (
        <p className="mt-4 font-sans text-sm text-tx-3">{t('insights.response.steps_empty')}</p>
      ) : (
        <ol className="mt-3 space-y-3">
          {steps.map((step) => (
            <Bar
              key={step.step}
              label={t('insights.response.step', { n: step.step + 1 })}
              value={t('insights.response.step_rate', {
                acknowledged: step.acknowledged,
                reached: step.reached,
                percent: formatPercent(step.reached ? step.acknowledged / step.reached : 0),
              })}
              share={step.reached ? step.acknowledged / step.reached : 0}
            />
          ))}
        </ol>
      )}
    </div>
  );
}

function AckVia({ via, t }: { via: Record<string, number>; t: TFn }) {
  const total = Object.values(via).reduce((sum, count) => sum + count, 0);
  const entries = Object.entries(via).sort(
    ([a], [b]) => VIA_ORDER.indexOf(a) - VIA_ORDER.indexOf(b) || a.localeCompare(b),
  );

  return (
    <div data-insights-via>
      <SectionHeading
        id="insights-via"
        title={t('insights.response.via_title')}
        description={t('insights.response.via_description')}
      />
      {total === 0 ? (
        <p className="mt-4 font-sans text-sm text-tx-3">{t('insights.response.via_empty')}</p>
      ) : (
        <ol className="mt-3 space-y-3">
          {entries.map(([key, count]) => (
            <Bar
              key={key}
              label={t(`insights.response.via.${key}`, { defaultValue: key })}
              value={`${count} · ${formatPercent(count / total)}`}
              share={count / total}
            />
          ))}
        </ol>
      )}
    </div>
  );
}

function Bar({ label, value, share }: { label: string; value: string; share: number }) {
  return (
    <li className="grid grid-cols-[minmax(0,1fr)_auto] gap-x-4 gap-y-1.5 font-sans text-sm">
      <span className="truncate font-strong text-tx-1">{label}</span>
      <span className="text-xs tabular-nums text-tx-2">{value}</span>
      <span className="col-span-2 h-1.5 overflow-hidden rounded-full bg-bg-2" aria-hidden>
        <span className="block h-full bg-indigo" style={{ width: `${Math.min(100, share * 100)}%` }} />
      </span>
    </li>
  );
}

function Breakdown({ breakdown, t }: { breakdown: ResponseInsights['breakdown']; t: TFn }) {
  const [dimension, setDimension] = React.useState<Dimension>('teams');
  const { nameOf } = useUsers();
  const teams = useTeams();
  const rows: BreakdownRow[] = breakdown[dimension];

  const name = (key: string) =>
    dimension === 'people'
      ? nameOf(key)
      : dimension === 'teams'
        ? teams.data?.find((team) => team.id === key)?.name ?? key
        : key;

  return (
    <div data-insights-breakdown className="space-y-3">
      <SectionHeading
        id="insights-breakdown"
        title={t('insights.response.breakdown_title')}
        description={t('insights.response.breakdown_description')}
      />
      <ViewTabs
        label={t('insights.response.breakdown_label')}
        value={dimension}
        onValueChange={setDimension}
        items={DIMENSIONS.map((key) => ({
          key,
          label: t(`insights.response.dimensions.${key}`),
          count: breakdown[key].length,
        }))}
      />
      {rows.length === 0 ? (
        <p className="font-sans text-sm text-tx-3">{t('insights.response.breakdown_empty')}</p>
      ) : (
        <Table className="[&_tr>*:first-child]:pl-0 [&_tr>*:last-child]:pr-0">
          <TableHeader>
            <TableRow>
              <TableHead>{t(`insights.response.columns.${dimension}`)}</TableHead>
              <TableHead className="text-right">{t('insights.response.columns.incidents')}</TableHead>
              <TableHead className="text-right">{t('insights.response.columns.mtta')}</TableHead>
              <TableHead className="text-right">{t('insights.response.columns.mttr')}</TableHead>
              <TableHead className="text-right">{t('insights.response.columns.escalation')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((row) => (
              <TableRow key={row.key} data-breakdown-row={row.key}>
                <TableCell className="max-w-[320px] truncate font-strong text-tx-1">{name(row.key)}</TableCell>
                <TableCell className="text-right tabular-nums">{row.incidents}</TableCell>
                <TableCell className="text-right tabular-nums">
                  {row.mttaSecs > 0 ? formatDuration(row.mttaSecs, t) : '—'}
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  {row.mttrSecs > 0 ? formatDuration(row.mttrSecs, t) : '—'}
                </TableCell>
                <TableCell className="text-right tabular-nums">{formatPercent(row.escalationRate)}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </div>
  );
}
