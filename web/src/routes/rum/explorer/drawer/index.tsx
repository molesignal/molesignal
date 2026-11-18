import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumField, RumRow } from '@/api/rum';
import { useTimeFormatter } from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import { ResizeHandle } from '@/shell/ResizeHandle';
import { Sheet, SheetContent, SheetTitle } from '@/shell/ui/sheet';

import { type RumTab, rowId, rowTitle } from '../model';
import { RumRowBody } from './Body';
import { entityOptions, NO_ROW, sessionHref } from './entity';
import { RUM_SUMMARY, titleField } from './facts';
import { type DrawerNeighbours, RumDrawerHead } from './Head';

export type { DrawerNeighbours } from './Head';

/** The row open in the drawer. */
export interface RumDrawerState {
  tab: RumTab;
  row: RumRow;
  open: boolean;
  /** Where the row happened, for a page that knows it better than the row it holds does. */
  span?: { from: number; to: number } | undefined;
  /** The request or the long task the waterfall of a view opens on, and when it happened, in microseconds. */
  focus?: string | undefined;
  focusAt?: number | undefined;
}

const MIN_WIDTH = 420;
/** The room the list keeps beside the drawer at the least. */
const LIST_ROOM = 48;
const DEFAULT_SHARE = 0.46;
/** A view opens wide: its waterfall is a chart with a panel beside it. */
const VIEW_SHARE = 0.8;

function useWindowWidth(): number {
  const [width, setWidth] = React.useState(() => (typeof window === 'undefined' ? 1440 : window.innerWidth));

  React.useEffect(() => {
    const measure = () => setWidth(window.innerWidth);
    window.addEventListener('resize', measure);
    return () => window.removeEventListener('resize', measure);
  }, []);

  return width;
}

/**
 * A row opened from the results without leaving them, in a drawer whose left edge the reader drags:
 * laid out as a log's drawer is, its head, what it says first, then its sections. The keys J and K,
 * or the arrows in its head, step to the next and the previous row of the list; closing it gives
 * the focus back to what opened it.
 */
export function RumDetailDrawer({
  state,
  fields,
  onClose,
  neighbours,
  returnFocusRef,
}: {
  state: RumDrawerState | null;
  fields: readonly RumField[];
  onClose: () => void;
  neighbours: DrawerNeighbours;
  /** What opened the drawer. */
  returnFocusRef: React.RefObject<HTMLElement | null>;
}) {
  const { t } = useTranslation('rum');
  const fmt = useTimeFormatter();
  const windowWidth = useWindowWidth();
  const maxWidth = Math.max(MIN_WIDTH, windowWidth - LIST_ROOM);
  const [chosen, setChosen] = React.useState<number | null>(null);
  const share = state?.tab === 'views' ? VIEW_SHARE : DEFAULT_SHARE;
  const width = Math.min(maxWidth, chosen ?? Math.max(MIN_WIDTH, Math.round(windowWidth * share)));

  // Read on the way down: a control inside that keeps its keys to itself still lets J and K by.
  const onKeyDownCapture = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    if (!event.currentTarget.contains(event.target as Node)) return;
    if ((event.target as HTMLElement).closest('input, textarea, [contenteditable="true"], [role="menu"]')) return;

    if (event.key === 'j' && neighbours.onNext) {
      event.preventDefault();
      neighbours.onNext();
    } else if (event.key === 'k' && neighbours.onPrevious) {
      event.preventDefault();
      neighbours.onPrevious();
    }
  };

  // A row opened by a page that holds no more than its id is named once it has been read whole.
  const whole = useQuery({ ...entityOptions(state?.tab ?? 'views', state?.row ?? NO_ROW, state?.span), enabled: Boolean(state?.open) });
  const row = whole.data ?? state?.row ?? NO_ROW;
  const named = state ? titleField(RUM_SUMMARY[state.tab], row) : null;
  const title = state ? (named ? String(row[named]) : rowTitle(state.tab, row) || t(`explorer.types.${state.tab}`)) : '';

  return (
    <Sheet open={Boolean(state?.open)} onOpenChange={(open) => !open && onClose()}>
      <SheetContent
        side="right"
        hideClose
        // Under the top bar, and light enough to keep the list in sight.
        overlayClassName="top-topbar bg-overlay-soft"
        aria-describedby={undefined}
        data-rum-drawer=""
        onKeyDownCapture={onKeyDownCapture}
        onCloseAutoFocus={(event) => {
          const opener = returnFocusRef.current;
          if (!opener?.isConnected) return;
          event.preventDefault();
          opener.focus();
        }}
        className={cn('bottom-0 top-topbar flex h-auto max-w-none flex-row gap-0 border-0 bg-[var(--functional-surface)] p-0 shadow-drawer sm:max-w-none')}
        style={{ width }}
      >
        <div className="absolute inset-y-0 left-0 z-30 -translate-x-1/2">
          <ResizeHandle
            label={t('explorer.drawer.resize')}
            width={width}
            min={MIN_WIDTH}
            max={maxWidth}
            grows="left"
            defaultWidth={Math.round(windowWidth * share)}
            onWidthChange={setChosen}
            quiet
            className="h-full"
          />
        </div>
        <SheetTitle className="sr-only">{t('explorer.drawer.title')}</SheetTitle>
        {state && (
          <div className="flex min-h-0 min-w-0 flex-1 flex-col">
            <RumDrawerHead
              title={title}
              kind={t(`explorer.types.${state.tab}`)}
              moment={fmt.micros(row.timestamp)}
              row={row}
              href={sessionHref(row)}
              neighbours={neighbours}
              onClose={onClose}
            />
            <RumRowBody
              key={`${state.tab}:${rowId(state.tab, state.row)}`}
              tab={state.tab}
              listed={state.row}
              fields={fields}
              span={state.span}
              focus={state.focus}
              focusAt={state.focusAt}
            />
          </div>
        )}
      </SheetContent>
    </Sheet>
  );
}
