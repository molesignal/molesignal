import { useTranslation } from 'react-i18next';

import { DataTable } from '@/admin';
import type { MonitorRevision, ProbeLocation, SyntheticResult } from '@/api/synthetics';
import { formatMicrosActive } from '@/lib/time';
import { cn } from '@/shell/lib/cn';

import { OutcomeMark } from '../../marks';
import { formatDuration, resultDurationMicros } from '../../model';

const LIST =
  'min-w-0 overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]';
const FLAT_TABLE = 'rounded-none border-0 bg-transparent';

/** Every run of one check, the latest first. A row opens the run's evidence. */
export function CheckRuns({
  results,
  locations,
  onOpen,
}: {
  results: readonly SyntheticResult[];
  locations: readonly ProbeLocation[];
  onOpen: (result: SyntheticResult) => void;
}) {
  const { t } = useTranslation('synthetics');

  return (
    <section data-check-runs className={cn(LIST, 'overflow-x-auto')}>
      <DataTable
        rows={[...results]}
        rowKey={(result) => result.id}
        onRowClick={onOpen}
        emptyLabel={t('states.no_results')}
        className={cn(FLAT_TABLE, 'min-w-[760px]')}
        columns={[
          {
            key: 'outcome',
            header: t('results.columns.outcome'),
            width: 130,
            cell: (result) => <OutcomeMark outcome={result.outcome} />,
          },
          {
            key: 'location',
            header: t('results.columns.location'),
            cell: (result) =>
              locations.find((location) => location.id === result.location_id)?.name ??
              result.location_id,
          },
          {
            key: 'started',
            header: t('results.columns.started'),
            width: 190,
            cell: (result) => (
              <span className="tabular-nums text-tx-1">{formatMicrosActive(result.started_at)}</span>
            ),
          },
          {
            key: 'duration',
            header: t('results.columns.duration'),
            width: 110,
            cell: (result) => (
              <span className="tabular-nums text-tx-1">
                {formatDuration(resultDurationMicros(result))}
              </span>
            ),
          },
          {
            key: 'attempts',
            header: t('results.columns.attempts'),
            width: 100,
            cell: (result) => <span className="tabular-nums text-tx-1">{result.attempts.length}</span>,
          },
          {
            key: 'run_type',
            header: t('results.columns.run_type'),
            width: 150,
            cell: (result) => (
              <span className="text-tx-2">
                {t(result.is_test ? 'results.test_run' : 'results.operational_run')}
              </span>
            ),
          },
        ]}
      />
    </section>
  );
}

/** The revisions a check has had, and which of them is the one on schedule. */
export function CheckRevisions({
  revisions,
  activeId,
  draftId,
}: {
  revisions: readonly MonitorRevision[];
  activeId: string | undefined;
  draftId: string | undefined;
}) {
  const { t } = useTranslation('synthetics');

  return (
    <section data-check-revisions className={LIST}>
      <DataTable
        rows={[...revisions]}
        rowKey={(revision) => revision.id}
        className={FLAT_TABLE}
        columns={[
          {
            key: 'number',
            header: t('detail.revision'),
            width: 110,
            cell: (revision) => <span className="font-strong text-tx-0">#{revision.number}</span>,
          },
          {
            key: 'status',
            header: t('checks.columns.status'),
            width: 170,
            cell: (revision) =>
              revision.id === activeId ? (
                <span className="font-strong text-green">{t('states.active')}</span>
              ) : revision.id === draftId ? (
                <span className="font-strong text-yellow">{t('states.draft')}</span>
              ) : (
                <span className="text-tx-2">{t('detail.previous_revision')}</span>
              ),
          },
          {
            key: 'created',
            header: t('detail.created'),
            cell: (revision) => (
              <span className="tabular-nums text-tx-1">{formatMicrosActive(revision.created_at)}</span>
            ),
          },
          {
            key: 'hash',
            header: t('detail.content_hash'),
            cell: (revision) => (
              <span className="font-code text-xs text-tx-2">{revision.content_hash.slice(0, 16)}</span>
            ),
          },
        ]}
      />
    </section>
  );
}
