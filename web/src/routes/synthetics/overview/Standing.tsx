import { useTranslation } from 'react-i18next';

import type { ProbeLocation, SyntheticResult } from '@/api/synthetics';
import { Readout } from '@/shell/readout/Readout';
import { SpectrumBar } from '@/shell/readout/SpectrumBar';
import { STATEMENT_CELL, Statement } from '@/shell/readout/Statement';

import type { CheckRow } from '../data';
import { formatPercent } from '../model';
import {
  STATES_WORST_FIRST,
  STATE_COLOR,
  availability,
  availabilityTone,
  availabilityTrend,
  checksVerdict,
  decidedRuns,
  runningChecks,
  stateCounts,
  type ChecksVerdict,
} from '../summary';

const VERDICT_TONE: Record<ChecksVerdict, { color: string; text: string }> = {
  failing: { color: 'var(--red)', text: 'text-red' },
  attention: { color: 'var(--yellow)', text: 'text-yellow' },
  passing: { color: 'var(--green)', text: 'text-green' },
  waiting: { color: 'var(--tx-3)', text: 'text-tx-0' },
  idle: { color: 'var(--tx-3)', text: 'text-tx-0' },
};

/**
 * What Synthetics opens with: are the checks passing, and can that be relied on.
 *
 * The verdict takes the colour of the worst check that is running, with how the
 * checks divide under it. Beside it are the three things the verdict rests on:
 * how the latest runs went, whether the places they run from are reachable, and
 * whether a failure would reach anyone.
 */
export function ChecksStanding({
  rows,
  results,
  locations,
}: {
  /** Every check that is not archived. */
  rows: readonly CheckRow[];
  /** The latest runs of those checks. */
  results: readonly SyntheticResult[];
  locations: readonly ProbeLocation[];
}) {
  const { t } = useTranslation('synthetics');
  const running = runningChecks(rows);
  const paused = rows.filter((row) => row.monitor.lifecycle === 'paused').length;
  const counts = stateCounts(running);
  const verdict = checksVerdict(counts);
  const tone = VERDICT_TONE[verdict];
  const runs = decidedRuns(results);
  const passRate = availability(results);
  const usable = locations.filter((location) => location.lifecycle !== 'archived');
  const online = usable.filter((location) => location.health === 'online');
  const notOnline = usable.filter((location) => location.health !== 'online');
  const paging = running.filter((row) => Boolean(row.revision?.escalation_policy_id)).length;

  const title =
    verdict === 'failing'
      ? t('standing.failing', { count: counts.failing })
      : verdict === 'attention'
        ? t('standing.attention', { count: counts.flaky + counts.degraded })
        : verdict === 'passing'
          ? t('standing.passing', { count: counts.healthy })
          : t(`standing.${verdict}`);
  const detail = [
    running.length > 0 && t('standing.running', { count: running.length }),
    paused > 0 && t('standing.paused', { count: paused }),
  ]
    .filter(Boolean)
    .join(' · ');

  return (
    <Statement
      label={t('standing.label')}
      tone={tone.color}
      eyebrow={t('standing.eyebrow')}
      title={title}
      titleClassName={tone.text}
      detail={detail || undefined}
      dataAttributes={{ 'data-checks-standing': verdict }}
      readouts={
        <>
          <Readout
            className={STATEMENT_CELL}
            riseIndex={1}
            label={t('standing.availability')}
            value={formatPercent(passRate)}
            tone={availabilityTone(passRate)}
            trend={runs.length > 1 ? { values: availabilityTrend(results) } : undefined}
            detail={
              runs.length > 0
                ? t('standing.of_runs', { count: runs.length })
                : t('standing.no_runs')
            }
          />
          <Readout
            className={STATEMENT_CELL}
            riseIndex={2}
            label={t('standing.locations')}
            value={
              usable.length > 0
                ? t('standing.of_total', { count: online.length, total: usable.length })
                : '—'
            }
            tone={notOnline.length > 0 ? 'warn' : undefined}
            // Which ones, not only how many: a name is what a reader acts on.
            detail={
              usable.length === 0
                ? t('standing.locations_none')
                : notOnline.length > 0
                  ? t('standing.locations_down', {
                      names: notOnline.map((location) => location.name).join(', '),
                    })
                  : t('standing.locations_all')
            }
          />
          <Readout
            className={STATEMENT_CELL}
            riseIndex={3}
            label={t('standing.paging')}
            value={
              running.length > 0
                ? t('standing.of_total', { count: paging, total: running.length })
                : '—'
            }
            tone={running.length > 0 && paging < running.length ? 'warn' : undefined}
            detail={
              running.length > 0 && paging === 0
                ? t('standing.paging_none')
                : t('standing.paging_detail')
            }
          />
        </>
      }
    >
      {running.length > 0 && (
        <SpectrumBar
          className="mt-[14px]"
          label={t('standing.states')}
          segments={STATES_WORST_FIRST.map((state) => ({
            key: state,
            label: t(`check_states.${state}`),
            value: counts[state],
            color: STATE_COLOR[state],
          }))}
        />
      )}
    </Statement>
  );
}
