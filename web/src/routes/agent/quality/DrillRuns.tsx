import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as drillsApi from '@/api/agent/drills';
import type { DrillRun, DrillSummary } from '@/api/agent/drills';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import { uiSectionTitleClass } from '@/shell/chrome';
import { FormDrawer } from '@/shell/FormDrawer';

import { failureNote, runChatLink, runGoing, runStatusLabel } from './drillsModel';
import { ScoreWord } from './ScoreWord';
import { DRILLS_KEY } from './state';

/** While a run is under way the runs are read again this often. */
const POLL_MS = 5_000;

/**
 * One drill: the incident and the root cause it is graded against, and its
 * runs, newest first, each with its score and the grading model's reason, or
 * why it has none, and a link to the investigation.
 */
export function DrillRuns({
  drill,
  threshold,
  onClose,
}: {
  drill: Pick<DrillSummary, 'id' | 'title' | 'reference' | 'source' | 'incidentId'> | null;
  threshold: number;
  onClose: () => void;
}) {
  const { t } = useTranslation('agent');
  const runs = useQuery({
    queryKey: [...DRILLS_KEY, 'runs', drill?.id],
    queryFn: () => drillsApi.listRuns(drill!.id),
    enabled: drill !== null,
    retry: false,
    refetchInterval: (query) => (query.state.data?.runs.some((run) => runGoing(run)) ? POLL_MS : false),
  });

  if (!drill) return null;

  return (
    <FormDrawer
      open
      onOpenChange={(open) => !open && onClose()}
      title={t('quality.drills.runs_title')}
      subtitle={drill.title}
      width={720}
    >
      <div data-quality-drill-runs={drill.id} className="flex flex-col gap-5 font-sans text-xs">
        <section aria-labelledby="drill-reference">
          <div className="flex flex-wrap items-baseline gap-x-3">
            <h3 id="drill-reference" className={`m-0 ${uiSectionTitleClass}`}>
              {t('quality.drills.reference')}
            </h3>
            <span className="text-tx-3">{t(`quality.drills.sources.${drill.source}`)}</span>
            <Link
              to={`/alerts/incidents/${encodeURIComponent(drill.incidentId)}`}
              className="ml-auto text-[var(--blue-ink)] hover:text-tx-0"
            >
              {t('quality.drills.open_incident')}
            </Link>
          </div>
          <p className="m-0 mt-1.5 whitespace-pre-wrap text-tx-1">{drill.reference}</p>
        </section>

        <section aria-labelledby="drill-runs">
          <h3 id="drill-runs" className={`m-0 ${uiSectionTitleClass}`}>
            {t('quality.drills.runs')}
          </h3>
          {runs.isPending ? (
            <ProductState variant="loading" />
          ) : runs.isError ? (
            <ProductState variant="error" error={runs.error} />
          ) : runs.data.runs.length === 0 ? (
            <p className="m-0 mt-2 text-tx-3">{t('quality.drills.no_runs')}</p>
          ) : (
            <ul className="m-0 mt-2 flex list-none flex-col gap-2 p-0">
              {runs.data.runs.map((run) => (
                <RunItem key={run.id} run={run} threshold={threshold} />
              ))}
            </ul>
          )}
        </section>
      </div>
    </FormDrawer>
  );
}

function RunItem({ run, threshold }: { run: DrillRun; threshold: number }) {
  const { t } = useTranslation('agent');
  const link = runChatLink(run);
  const answeredBy = [run.model, run.promptVersion === null ? null : t('quality.drills.prompt_version', { version: run.promptVersion })]
    .filter((part): part is string => Boolean(part))
    .join(' · ');

  return (
    <li data-quality-drill-run={run.id} className="rounded-md bg-bg-2 px-3 py-2">
      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
        {run.status === 'done' ? (
          <ScoreWord score={run.score} threshold={threshold} className="text-sm" />
        ) : (
          <span className={run.status === 'failed' ? 'text-[var(--red-ink)]' : 'text-[var(--blue-ink)]'}>
            {runStatusLabel(t, run)}
          </span>
        )}
        <span className="text-tx-3">{formatMicrosActive(run.createdAt)}</span>
        {answeredBy && <span className="text-tx-2">{answeredBy}</span>}
        {link && (
          <Link to={link} className="ml-auto text-[var(--blue-ink)] hover:text-tx-0">
            {t('quality.drills.open_run')}
          </Link>
        )}
      </div>
      {run.status === 'done' && run.reasoning && (
        <p data-quality-drill-reasoning="" className="m-0 mt-1.5 text-tx-1">
          {run.reasoning}
        </p>
      )}
      {run.status === 'failed' && (
        <p className="m-0 mt-1.5 text-tx-1">
          {failureNote(t, run)}
          {run.detail && <span className="ml-2 text-tx-3">{run.detail}</span>}
        </p>
      )}
    </li>
  );
}
