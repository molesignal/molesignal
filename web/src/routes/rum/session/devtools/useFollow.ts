import * as React from 'react';

import { lastBefore } from './model';
import { type ReplayController, replayClockMicros, useReplaySelector } from '../../replay/controller';

/** The row the replay is at: the last that had happened by its moment; -1 before them all, and where there is no replay. */
export function useLitRow(controller: ReplayController, rows: readonly { at: number }[]): number {
  return useReplaySelector(controller, (state) => {
    const clock = replayClockMicros(state);

    return clock === null ? -1 : lastBefore(rows, clock);
  });
}

/** Whether a replay is there to be moved. */
export const useSeekable = (controller: ReplayController): boolean => useReplaySelector(controller, (state) => state.seekable);

/**
 * Keeps the lit row of a list in sight as the replay moves it, by moving the list and nothing else: the
 * page under it stays where the reader left it. A row that is already in sight does not move the list,
 * and the head that stays over the rows does not hide one.
 */
export function useKeepInSight(list: React.RefObject<HTMLElement | null>, lit: number) {
  React.useEffect(() => {
    const box = list.current;
    const row = box?.querySelector<HTMLElement>('[data-current="true"]');

    if (!box || !row) return;

    const head = box.querySelector<HTMLElement>('[data-list-head]')?.offsetHeight ?? 0;
    const around = box.getBoundingClientRect();
    const found = row.getBoundingClientRect();

    if (found.top < around.top + head) box.scrollTop -= around.top + head - found.top;
    else if (found.bottom > around.bottom) box.scrollTop += found.bottom - around.bottom;
  }, [list, lit]);
}
