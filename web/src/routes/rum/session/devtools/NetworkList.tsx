import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

import { type Axis, barOf, type NetworkRow } from './model';
import { HEAD, ListScroller, LONG_LIST, Moment, NoRows, rowClass } from './parts';
import { useKeepInSight, useLitRow, useSeekable } from './useFollow';
import { formatDurationMs, formatSize } from '../../_helpers';
import { formatClock } from '../../replay/clock';
import type { ReplayController } from '../../replay/controller';
import { statusInk, typeInk } from '../../waterfall/ink';

const TRACKS = '64px 48px minmax(0,1fr) 44px 64px 64px 64px';

/**
 * The requests of the session in the order they were made: when, how, what, how it was answered, what
 * kind it was, how long it took and how big it was. Where a view is chosen each also has its bar on the
 * time the view's requests took, the same ruler for every row. The row the replay is at is lit, and a
 * row moves the replay to its moment.
 */
export function NetworkList({
  rows,
  axis,
  controller,
  zero,
  empty,
  notice,
}: {
  rows: readonly NetworkRow[];
  /** The ruler of the bars, when the rows are of one view. */
  axis: Axis | null;
  controller: ReplayController;
  /** Where the replay's ruler starts, in microseconds since the epoch. */
  zero: number;
  empty: string;
  /** What is said under the rows, when there is something to say. */
  notice?: string | undefined;
}) {
  const { t } = useTranslation('rum');
  const list = React.useRef<HTMLDivElement>(null);
  const lit = useLitRow(controller, rows);
  const seekable = useSeekable(controller);
  const tracks = axis ? `${TRACKS} 120px` : TRACKS;
  const long = rows.length > LONG_LIST;

  useKeepInSight(list, lit);

  if (rows.length === 0) return <NoRows>{empty}</NoRows>;

  return (
    <>
      <ListScroller ref={list} label={t('session_detail.tools.network.aria')} minWidth={axis ? 700 : 560}>
        <div role="row" data-list-head="" className={HEAD} style={{ gridTemplateColumns: tracks }}>
          <span role="columnheader">{t('session_detail.tools.network.time')}</span>
          <span role="columnheader">{t('session_detail.tools.network.method')}</span>
          <span role="columnheader">{t('session_detail.tools.network.name')}</span>
          <span role="columnheader">{t('session_detail.tools.network.status')}</span>
          <span role="columnheader">{t('session_detail.tools.network.type')}</span>
          <span role="columnheader" className="text-right">
            {t('session_detail.tools.network.duration')}
          </span>
          <span role="columnheader" className="text-right">
            {t('session_detail.tools.network.size')}
          </span>
          {axis && <span role="columnheader">{`0 – ${formatDurationMs(axis.length / 1_000)}`}</span>}
        </div>
        {rows.map((row, index) => {
          const bar = axis ? barOf(row, axis) : null;

          return (
            <div
              key={row.key}
              role="row"
              data-rum-request={row.key}
              data-current={lit === index ? 'true' : undefined}
              data-failed={row.failed ? '' : undefined}
              className={rowClass(lit === index, long)}
              style={{ gridTemplateColumns: tracks }}
            >
              <Moment
                at={row.at}
                zero={zero}
                seekable={seekable}
                label={t('session_detail.tools.seek', { time: formatClock((row.at - zero) / 1_000, true) })}
                onSeek={() => controller.seekToMicros(row.at)}
              />
              <span role="cell" className="font-mono text-tx-2">
                {row.method ?? '—'}
              </span>
              <span role="cell" className="flex min-w-0 items-baseline gap-1.5" title={row.url}>
                {row.host && <span className="shrink-[3] truncate text-tx-3">{row.host}</span>}
                <span className="min-w-0 truncate text-tx-0">{row.path}</span>
              </span>
              <span role="cell" className={cn('font-mono', statusInk(row.status))}>
                {row.status ?? '—'}
              </span>
              <span role="cell" className={cn('truncate', typeInk(row.type))}>
                {row.type}
              </span>
              <span role="cell" className="text-right tabular-nums text-tx-1">
                {row.duration === null ? '—' : formatDurationMs(row.duration)}
              </span>
              <span role="cell" className="text-right tabular-nums text-tx-2">
                {row.size === null ? '—' : formatSize(row.size)}
              </span>
              {bar && (
                <span role="cell" aria-hidden className="relative block h-2">
                  <span
                    className="absolute inset-y-0 min-w-[2px] rounded-[2px] opacity-85"
                    style={{ left: `${bar.left * 100}%`, width: `${bar.width * 100}%`, background: row.failed ? 'var(--red)' : 'var(--blue)' }}
                  />
                </span>
              )}
            </div>
          );
        })}
      </ListScroller>
      {notice && (
        <p data-rum-tool-notice="" className="m-0 border-t border-[var(--row-rule)] px-3 py-2 font-sans text-xs text-tx-3">
          {notice}
        </p>
      )}
    </>
  );
}
