import { useQuery } from '@tanstack/react-query';
import { Lightbulb, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as queryApi from '@/api/query';
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/shell/ui/popover';
import type { QueryLanguage, QueryResult } from '@/types/query';

/**
 * Advisory query-optimization tips, derived from the last query's stats via
 * `POST /query/recommendations`. Stateless and non-blocking — it never re-runs
 * the query, only analyzes `scanned_rows` / `took_ms` / row count + time range.
 * Shared across the Logs (SQL) and Metrics / PromQL builder (PromQL) pages.
 *
 * Renders nothing when there are no tips; dismissible per query (the dismissal
 * is keyed on the result fingerprint, so the next run re-shows tips).
 */
export function QueryRecommendations({
  result,
  statement,
  language,
  timeRangeSecs,
  className,
  variant = 'card',
}: {
  result: QueryResult | null | undefined;
  statement: string;
  language: QueryLanguage;
  timeRangeSecs: number | null;
  className?: string;
  variant?: 'card' | 'inline';
}) {
  const { t } = useTranslation('common');
  const profileKey = result
    ? `${language}|${statement}|${result.scanned_rows}|${result.took_ms}|${result.rows.length}`
    : '';
  const [dismissedKey, setDismissedKey] = React.useState('');

  const recsQuery = useQuery({
    queryKey: ['query-recommendations', profileKey],
    enabled: Boolean(result && statement),
    staleTime: 60_000,
    retry: false,
    queryFn: () =>
      queryApi.recommendations({
        language,
        statement,
        scanned_rows: result!.scanned_rows,
        returned_rows: result!.rows.length,
        took_ms: result!.took_ms,
        ...(timeRangeSecs != null ? { time_range_secs: timeRangeSecs } : {}),
      }),
  });

  const recs = recsQuery.data?.recommendations ?? [];
  if (!result || recs.length === 0 || dismissedKey === profileKey) return null;

  if (variant === 'inline') {
    const first = recs[0]!;
    return (
      <div className={`flex min-h-9 items-center gap-2 border-b border-bd-0 bg-bg-1 px-3 font-sans text-xs ${className ?? ''}`}>
        <Lightbulb className="h-3.5 w-3.5 shrink-0 text-yellow" />
        <span className="min-w-0 truncate text-tx-2">
          <span className="font-semibold text-tx-1">
            {t('query_recommendations.inline_title', { defaultValue: 'Query can be optimized:' })}
          </span>{' '}
          {first.title}
        </span>
        <Popover>
          <PopoverTrigger asChild>
            <button
              type="button"
              className="ml-auto shrink-0 rounded px-2 py-1 font-semibold text-indigo-soft hover:bg-bg-3"
            >
              {t('query_recommendations.view', { defaultValue: 'View tips' })}
            </button>
          </PopoverTrigger>
          <PopoverContent align="end" className="w-[420px] max-w-[calc(100vw-32px)] p-3">
            <div className="font-sans text-xs font-bold text-tx-0">
              {t('query_recommendations.title', { defaultValue: 'Optimization tips' })}
            </div>
            <ul className="mt-2 flex flex-col gap-2">
              {recs.map((recommendation) => (
                <li key={recommendation.code} className="flex items-start gap-2 font-sans text-xs leading-snug">
                  <span className={`mt-1 h-1.5 w-1.5 shrink-0 rounded-full ${severityDotClass(recommendation.severity)}`} />
                  <span className="min-w-0">
                    <span className="font-bold text-tx-1">{recommendation.title}</span>
                    <span className="text-tx-3"> — {recommendation.detail}</span>
                  </span>
                </li>
              ))}
            </ul>
          </PopoverContent>
        </Popover>
        <button
          type="button"
          onClick={() => setDismissedKey(profileKey)}
          aria-label={t('query_recommendations.dismiss', { defaultValue: 'Dismiss tips' })}
          className="grid h-7 w-7 shrink-0 place-items-center rounded text-tx-3 hover:bg-bg-3 hover:text-tx-0"
        >
          <X className="h-3 w-3" />
        </button>
      </div>
    );
  }

  return (
    <div className={`rounded-md border border-bd-1 bg-bg-1 px-3 py-2 ${className ?? ''}`}>
      <div className="flex items-center gap-2">
        <Lightbulb className="h-3.5 w-3.5 text-yellow" />
        <span className="font-sans text-xs font-bold uppercase tracking-wide text-tx-2">
          {t('query_recommendations.title', { defaultValue: 'Optimization tips' })}
        </span>
        <button
          type="button"
          onClick={() => setDismissedKey(profileKey)}
          aria-label={t('query_recommendations.dismiss', { defaultValue: 'Dismiss tips' })}
          className="ml-auto grid h-8 w-8 place-items-center rounded-md text-tx-3 hover:bg-bg-3 hover:text-tx-0"
        >
          <X className="h-3 w-3" />
        </button>
      </div>
      <ul className="mt-1.5 flex flex-col gap-1">
        {recs.map((r) => (
          <li key={r.code} className="flex items-start gap-2 font-sans text-xs leading-snug">
            <span className={`mt-1 h-1.5 w-1.5 shrink-0 rounded-full ${severityDotClass(r.severity)}`} />
            <span className="min-w-0">
              <span className="font-bold text-tx-1">{r.title}</span>
              <span className="text-tx-3"> — {r.detail}</span>
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function severityDotClass(severity: string): string {
  if (severity === 'critical') return 'bg-red';
  if (severity === 'warning') return 'bg-orange';
  return 'bg-blue';
}
