import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useParams, useSearchParams } from 'react-router-dom';

import type { TraceWindow } from '@/api/web';
import { type ProductStateProps } from '@/product/states';
import { DetailPage } from '@/product/templates';
import { COMPARE_PARAMS } from '@/viz/trace/compare/link';
import { CompareActions, TraceCompare } from '@/viz/trace/compare/TraceCompare';
import { TraceView } from '@/viz/trace/view/TraceView';

/**
 * A moment of a link to the trace, in epoch microseconds: links from the
 * list carry microseconds, links from the other signals carry ISO moments.
 */
function micros(value: string | null): number | null {
  if (!value) return null;
  if (/^\d+$/.test(value)) return Number(value);
  const ms = Date.parse(value);
  return Number.isNaN(ms) ? null : ms * 1_000;
}

/** Where the trace is looked for, when the link that led here says. */
export function traceWindowFromParams(params: URLSearchParams, keys: { from: string; to: string } = { from: 'from', to: 'to' }): TraceWindow | undefined {
  const from = micros(params.get(keys.from));
  const to = micros(params.get(keys.to));
  return from !== null && to !== null && to > from ? { from, to } : undefined;
}

/**
 * A trace's own page: its whole view, as tall as the window, the summary
 * fixed over a waterfall that scrolls on its own. The span picked is kept in
 * the address, so a link to the page opens on it. The trace's id, its logs,
 * metrics, profiles and session are said once, in the head of the trace.
 * With another trace named to compare it with, the page sets the two side by
 * side instead.
 */
export function TraceDetail() {
  const { t } = useTranslation('traces');
  const { id } = useParams<{ id: string }>();
  const [searchParams, setSearchParams] = useSearchParams();
  // The span asked for when the page opened; picking another one is not a new page.
  const [initialSpanId] = React.useState(() => searchParams.get('spanId'));
  const window = React.useMemo(() => traceWindowFromParams(searchParams), [searchParams]);
  const compared = searchParams.get(COMPARE_PARAMS.trace)?.trim() || null;
  const comparedWindow = React.useMemo(() => traceWindowFromParams(searchParams, COMPARE_PARAMS), [searchParams]);
  const paramsRef = React.useRef(searchParams);
  paramsRef.current = searchParams;

  const state: ProductStateProps | null = id
    ? null
    : { variant: 'empty', title: t('detail.missing_title'), description: t('detail.missing_description') };

  const keepSpan = React.useCallback(
    (spanId: string | null) => {
      const next = new URLSearchParams(paramsRef.current);
      if (spanId) next.set('spanId', spanId);
      else next.delete('spanId');
      next.delete('spanId');
      if (next.toString() !== paramsRef.current.toString()) setSearchParams(next, { replace: true });
    },
    [setSearchParams],
  );

  return (
    <DetailPage
      title={t(compared ? 'compare.page_title' : 'detail.title')}
      // A comparison names itself in the trail as in its title.
      breadcrumbs={
        compared ? [{ labelKey: 'traces', to: '/traces' }, { labelKey: 'breadcrumbs.trace_detail', label: t('compare.page_title') }] : undefined
      }
      toolbar={id && compared ? <CompareActions a={{ traceId: id, window }} b={{ traceId: compared, window: comparedWindow }} /> : undefined}
      appearance="surface"
      className="flex h-full min-h-0 flex-col"
      bodyClassName="flex min-h-0 flex-1 flex-col pb-[12px]"
      state={state}
    >
      {id && (
        <div className="min-h-0 flex-1 overflow-hidden rounded-md bg-[var(--functional-surface)]" data-trace-page="">
          {compared ? (
            <TraceCompare a={{ traceId: id, window }} b={{ traceId: compared, window: comparedWindow }} />
          ) : (
            <TraceView
              traceId={id}
              window={window}
              initialSpanId={initialSpanId}
              frame="page"
              onSpanSelect={(span) => keepSpan(span?.spanId ?? null)}
            />
          )}
        </div>
      )}
    </DetailPage>
  );
}
