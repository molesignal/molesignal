import { useQuery } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import * as savedViewsApi from '@/api/savedViews';
import type { SavedView } from '@/api/savedViews';
import { toApiError } from '@/lib/http';
import { activeTimeZone, formatMicrosActive, tzOffsetLabel } from '@/lib/time';
import {
  ReportDocument,
  ReportUnavailable,
  type ReportFact,
} from '@/report-render/Document';
import { useReportRenderState } from '@/report-render/readiness';
import { timeFromSearchParams } from '@/shell/UrlHydration';
import { useAuthStore } from '@/stores/auth';

import {
  NoStreamError,
  reportRange,
  runSavedView,
  type ReportRange,
  type SavedViewResult,
} from './query';
import { ResultTable } from './ResultTable';

/** The query parameter naming the saved view a report prints. */
const VIEW_PARAM = 'view';

const LANGUAGE_NAME: Record<SavedView['language'], string> = {
  sql: 'SQL',
  promql: 'PromQL',
};

/**
 * A saved view as a report prints it: its name, what the rows cover, the
 * statement, and the rows the statement returns over the report's range.
 *
 * The renderer is told when the rows are in, and is told why when the view
 * cannot be run, so a report is never a picture of a page still loading or of
 * an error.
 */
export function SavedViewReport() {
  const { t } = useTranslation('saved-views');
  const [search] = useSearchParams();
  const id = search.get(VIEW_PARAM)?.trim() ?? '';
  const view = useQuery({
    queryKey: ['saved-views', 'get', id],
    queryFn: () => savedViewsApi.get(id),
    enabled: Boolean(id),
  });

  if (!id) return <ReportUnavailable reason={t('report.no_view')} />;
  if (view.error) return <ReportUnavailable reason={failureReason(view.error, t)} />;
  if (!view.data) return null;

  // Keyed by view, so one that was ready does not vouch for the next.
  return <SavedViewRun key={view.data.id} view={view.data} />;
}

/** Runs the view once, over the range the report covers. */
function SavedViewRun({ view }: { view: SavedView }) {
  const { t } = useTranslation('saved-views');
  const [search] = useSearchParams();
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const [generatedAt] = React.useState(() => new Date());
  const covered = React.useMemo<{ range: ReportRange | null; error: unknown }>(() => {
    try {
      return {
        range: reportRange(view, timeFromSearchParams(search).window, generatedAt),
        error: null,
      };
    } catch (error) {
      return { range: null, error };
    }
  }, [generatedAt, search, view]);
  const { range } = covered;
  const result = useQuery({
    queryKey: ['saved-views', 'report', view.id, view.updated_at, orgId, range?.start, range?.end],
    queryFn: () => runSavedView(view, orgId, range!),
    enabled: range !== null,
    // Printed once: the rows on the page are the rows that were asked for.
    staleTime: Infinity,
  });

  if (!range) return <ReportUnavailable reason={failureReason(covered.error, t)} />;
  if (result.error) return <ReportUnavailable reason={failureReason(result.error, t)} />;

  return (
    <SavedViewDocument view={view} range={range} result={result.data} generatedAt={generatedAt} />
  );
}

function SavedViewDocument({
  view,
  range,
  result,
  generatedAt,
}: {
  view: SavedView;
  range: ReportRange;
  result: SavedViewResult | undefined;
  generatedAt: Date;
}) {
  const { t } = useTranslation('saved-views');

  useReportRenderState(result ? 'ready' : 'loading');

  const facts = React.useMemo<ReportFact[]>(() => {
    const moment = (micros: number) => formatMicrosActive(micros, false);

    return [
      { label: t('report.time_range'), value: `${moment(range.start)} – ${moment(range.end)}` },
      { label: t('report.timezone'), value: timezoneText(activeTimeZone()) },
      ...(view.stream ? [{ label: t('report.stream'), value: view.stream }] : []),
      ...(result ? [{ label: t('report.rows'), value: rowsText(result, t) }] : []),
      { label: t('report.generated'), value: moment(generatedAt.getTime() * 1_000) },
    ];
  }, [generatedAt, range, result, t, view.stream]);

  return (
    <ReportDocument title={view.name} facts={facts}>
      <figure className="mb-[16px]">
        <figcaption className="type-label mb-[6px] font-medium text-tx-3">
          {LANGUAGE_NAME[view.language]}
        </figcaption>
        <pre className="whitespace-pre-wrap rounded-md border border-bd-0 bg-bg-1 px-[12px] py-[10px] font-mono text-xs leading-5 text-tx-1 [overflow-wrap:anywhere]">
          {view.statement}
        </pre>
      </figure>
      {result &&
        (result.rows.length > 0 ? (
          <ResultTable columns={result.columns} rows={result.rows} label={t('report.results')} />
        ) : (
          <p className="font-sans text-sm text-tx-2">{t('report.empty')}</p>
        ))}
    </ReportDocument>
  );
}

/** How many rows are on the page, said as a limit when the statement returns more. */
function rowsText(result: SavedViewResult, t: TFunction<'saved-views'>): string {
  return result.truncated
    ? t('report.rows_capped', { shown: result.rows.length })
    : String(result.rows.length);
}

/** A zone by its name and its offset, said once when the name is the offset. */
function timezoneText(timezone: string): string {
  const offset = tzOffsetLabel(timezone);

  return offset === timezone ? timezone : `${timezone} · ${offset}`;
}

function failureReason(error: unknown, t: TFunction<'saved-views'>): string {
  if (error instanceof NoStreamError) return t('report.no_stream');

  return toApiError(error).message;
}
