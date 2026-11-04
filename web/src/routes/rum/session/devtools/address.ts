import { type Display, parseDisplay, parseShow, parseTab, type Show, type ToolTab } from './model';
import { type Changes, useAddressState, written } from '../../performance/useAddress';

/** What the tools under the replay show, as the address of the session says it. */
export interface ToolsState {
  /** The tab chosen; none leaves it to the session, which opens on its errors where it has any. */
  tab: ToolTab | null;
  /** The view the rows are of: `all`, the id of one, or none for the one the replay is in. */
  view: string | null;
  /** How the requests are shown. */
  display: Display;
  /** Which of the requests of a list are shown. */
  show: Show;
}

export function readState(params: URLSearchParams): ToolsState {
  return { tab: parseTab(params.get('tab')), view: params.get('view') || null, display: parseDisplay(params.get('display')), show: parseShow(params.get('show')) };
}

/** The address after a change: what the tools open on is left out of it. */
export function applyChange(current: URLSearchParams, changes: Changes): URLSearchParams {
  const next = written(current, changes);

  if (next.get('display') === 'list') next.delete('display');
  if (next.get('show') === 'all') next.delete('show');

  return next;
}

export const useToolsState = () => useAddressState(readState, applyChange);
