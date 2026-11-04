import type { Changes } from './useAddress';

/** The view open in the drawer of a page, as its address says it: which, when it began (to find it), and what to open it on. */
export interface OpenView {
  id: string;
  /** When the thing that led here happened, in microseconds: the view began before it. */
  at: number | null;
  /** The request or the long task its waterfall opens on. */
  focus: string;
}

export function readOpenView(params: URLSearchParams): OpenView | null {
  const id = params.get('view');

  if (!id) return null;

  const at = Number(params.get('at'));

  return { id, at: Number.isFinite(at) && at > 0 ? at : null, focus: params.get('focus') ?? '' };
}

/** What opening a view of a request or a long task changes in an address. */
export const openViewChange = (id: string, at: number | null, focus: string): Changes => ({
  view: id,
  at: at === null ? null : String(Math.round(at)),
  focus: focus || null,
});

export const CLOSE_VIEW: Changes = { view: null, at: null, focus: null };
