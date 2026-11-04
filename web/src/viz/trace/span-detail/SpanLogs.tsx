import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as streamsApi from '@/api/streams';
import * as webApi from '@/api/web';
import { useTimeFormatter, zonedMoment } from '@/lib/time';
import { logFieldFor } from '@/routes/logs/conditions/model';
import { logLevelLabel, logLevelTone, primaryLogMessage } from '@/routes/logs/viewModel';
import { cn } from '@/shell/lib/cn';
import { ViewTabs } from '@/shell/tabs/ViewTabs';

/** The most log streams a trace's logs are looked for in. */
const SEARCHED_STREAMS = 3;
/** The most lines shown; the rest are a link away. */
const SHOWN_LINES = 50;

const LEVEL_CLASS: Record<ReturnType<typeof logLevelTone>, string> = {
  error: 'text-[var(--red-ink)]',
  warn: 'text-[var(--yellow-ink)]',
  info: 'text-[var(--blue-ink)]',
  quiet: 'text-tx-3',
};

interface LogLine {
  stream: string;
  micros: number;
  level: string;
  message: string;
}

/**
 * The log lines written during the span, or during the whole trace, right
 * beside it: each log stream that keeps a trace id is asked by its own field
 * for it, as the Logs page asks. The full search is a link away.
 */
export function SpanLogs({
  traceId,
  spanId,
  window,
  traceLogsHref,
  spanLogsHref,
}: {
  traceId: string;
  spanId: string;
  /** When the trace ran, in epoch microseconds, a little widened. */
  window: webApi.TraceWindow;
  traceLogsHref: string | undefined;
  spanLogsHref: string | undefined;
}) {
  const { t } = useTranslation('traces');
  const [scope, setScope] = React.useState<'span' | 'trace'>('span');
  const fmt = useTimeFormatter();

  const streams = useQuery({
    queryKey: ['streams', 'trace-logs'],
    queryFn: () => streamsApi.list(500),
    staleTime: 60_000,
  });

  const searched = React.useMemo(
    () =>
      (streams.data ?? [])
        .filter((stream) => stream.type === 'logs' && streamsApi.isQueryable(stream))
        .map((stream) => ({
          name: stream.name,
          trace: logFieldFor('trace_id', stream.schema.fields),
          span: logFieldFor('span_id', stream.schema.fields),
        }))
        .filter((stream) => stream.trace !== null)
        .slice(0, SEARCHED_STREAMS),
    [streams.data],
  );

  const lines = useQuery({
    queryKey: ['web', 'trace-logs', traceId, scope === 'span' ? spanId : null, searched.map((stream) => stream.name), window.from, window.to],
    enabled: searched.length > 0,
    staleTime: 30_000,
    queryFn: async () => {
      const pages = await Promise.all(
        searched
          .filter((stream) => scope === 'trace' || stream.span !== null)
          .map(async (stream) => {
            const filters: webApi.LogListFilter[] = [{ field: stream.trace!.name, op: '=', value: traceId, quoted: true }];
            if (scope === 'span' && stream.span) filters.push({ field: stream.span.name, op: '=', value: spanId, quoted: true });

            const page = await webApi.logs({ stream: stream.name, from: window.from, to: window.to, filters, limit: SHOWN_LINES });
            return (page.items ?? []).map((record): LogLine => ({
              stream: stream.name,
              micros: Number(record._timestamp) || 0,
              level: logLevelLabel(record, ''),
              message: primaryLogMessage(record).value,
            }));
          }),
      );

      return pages.flat().sort((left, right) => left.micros - right.micros).slice(0, SHOWN_LINES);
    },
  });

  const href = scope === 'span' ? spanLogsHref : traceLogsHref;
  const several = new Set((lines.data ?? []).map((line) => line.stream)).size > 1;

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-2">
        <ViewTabs
          label={t('view.logs.scope')}
          value={scope}
          onValueChange={setScope}
          items={[
            { key: 'span', label: t('view.logs.span') },
            { key: 'trace', label: t('view.logs.trace') },
          ]}
        />
        {href && (
          <Link to={href} className="shrink-0 font-sans text-xs text-[var(--blue-ink)] hover:underline focus-visible:underline focus-visible:outline-none">
            {t('view.logs.open')}
          </Link>
        )}
      </div>

      {streams.isLoading || lines.isLoading ? (
        <p className="px-[6px] font-sans text-xs text-tx-3">{t('view.logs.loading')}</p>
      ) : searched.length === 0 ? (
        <p className="px-[6px] font-sans text-xs text-tx-3">{t('view.logs.no_stream')}</p>
      ) : lines.isError ? (
        <p className="px-[6px] font-sans text-xs text-[var(--red-ink)]">{t('view.logs.failed')}</p>
      ) : (lines.data ?? []).length === 0 ? (
        <p className="px-[6px] font-sans text-xs text-tx-3">{t(scope === 'span' ? 'view.logs.none_span' : 'view.logs.none_trace')}</p>
      ) : (
        <ol data-testid="trace-span-logs" className="divide-y divide-bd-0 rounded-md bg-bg-1">
          {(lines.data ?? []).map((line, index) => {
            const moment = zonedMoment(Math.floor(line.micros / 1_000), fmt.tz);

            return (
              <li key={`${line.stream}-${line.micros}-${index}`} className="px-[10px] py-[6px]">
                <div className="flex items-center gap-2 font-sans text-xs tabular-nums">
                  <span className="text-tx-3">{moment.isValid() ? moment.format('HH:mm:ss.SSS') : '—'}</span>
                  {line.level && <span className={cn('font-strong', LEVEL_CLASS[logLevelTone(line.level)])}>{line.level}</span>}
                  {several && <span className="min-w-0 truncate text-tx-3">{line.stream}</span>}
                </div>
                <p className="mt-0.5 line-clamp-3 whitespace-pre-wrap break-words font-sans text-xs text-tx-0 [overflow-wrap:anywhere]">
                  {line.message || '—'}
                </p>
              </li>
            );
          })}
        </ol>
      )}
    </div>
  );
}
