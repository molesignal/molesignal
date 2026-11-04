import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

import { type LogRow } from './model';
import { HEAD, ListScroller, LONG_LIST, Moment, NoRows, rowClass } from './parts';
import { useKeepInSight, useLitRow, useSeekable } from './useFollow';
import { formatClock } from '../../replay/clock';
import type { ReplayController } from '../../replay/controller';

const TRACKS = '64px 56px minmax(0,1fr) minmax(64px,120px)';

const LEVEL_INK: Record<string, string> = {
  error: 'text-[var(--red-ink)]',
  warn: 'text-[var(--orange-ink)]',
  info: 'text-tx-1',
  debug: 'text-tx-3',
};

/**
 * What the page and the application wrote, in the order it happened: when, how serious and what it
 * said. The row the replay is at is lit, and a row moves the replay to its moment.
 */
export function LogList({
  rows,
  controller,
  zero,
  empty,
  notice,
}: {
  rows: readonly LogRow[];
  controller: ReplayController;
  zero: number;
  empty: string;
  notice?: string | undefined;
}) {
  const { t } = useTranslation('rum');
  const list = React.useRef<HTMLDivElement>(null);
  const lit = useLitRow(controller, rows);
  const seekable = useSeekable(controller);
  const long = rows.length > LONG_LIST;

  useKeepInSight(list, lit);

  if (rows.length === 0) return <NoRows>{empty}</NoRows>;

  return (
    <>
      <ListScroller ref={list} label={t('session_detail.tools.logs.aria')} minWidth={480}>
        <div role="row" data-list-head="" className={HEAD} style={{ gridTemplateColumns: TRACKS }}>
          <span role="columnheader">{t('session_detail.tools.logs.time')}</span>
          <span role="columnheader">{t('session_detail.tools.logs.level')}</span>
          <span role="columnheader">{t('session_detail.tools.logs.message')}</span>
          <span role="columnheader">{t('session_detail.tools.logs.logger')}</span>
        </div>
        {rows.map((row, index) => (
          <div
            key={row.key}
            role="row"
            data-rum-log={row.level}
            data-current={lit === index ? 'true' : undefined}
            className={rowClass(lit === index, long)}
            style={{ gridTemplateColumns: TRACKS }}
          >
            <Moment
              at={row.at}
              zero={zero}
              seekable={seekable}
              label={t('session_detail.tools.seek', { time: formatClock((row.at - zero) / 1_000, true) })}
              onSeek={() => controller.seekToMicros(row.at)}
            />
            <span role="cell" className={cn('min-w-0 truncate font-strong', LEVEL_INK[row.level] ?? 'text-tx-1')}>
              {row.level}
            </span>
            <span role="cell" className="min-w-0 truncate font-mono text-tx-0" title={row.stack ?? row.message}>
              {row.message}
            </span>
            <span role="cell" className="min-w-0 truncate text-tx-3" title={row.logger ?? undefined}>
              {row.logger ?? '—'}
            </span>
          </div>
        ))}
      </ListScroller>
      {notice && (
        <p data-rum-tool-notice="" className="m-0 border-t border-[var(--row-rule)] px-3 py-2 font-sans text-xs text-tx-3">
          {notice}
        </p>
      )}
    </>
  );
}
