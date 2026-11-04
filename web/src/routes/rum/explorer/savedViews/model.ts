/** A view of the explorer kept to come back to: its name, and the address that shows it. */
export interface RumSavedView {
  id: string;
  name: string;
  search: string;
}

/** The most views one keeps, and the longest a name and an address of one are, as the server keeps them. */
export const MAX_SAVED_VIEWS = 30;
export const MAX_VIEW_NAME = 64;
export const MAX_VIEW_SEARCH = 2_048;

/** Where the views are kept: one list per person and organization. */
export const SAVED_VIEWS_SCOPE = 'rum.saved_views';
export const SAVED_VIEWS_KEY = 'explorer';

export interface RumSavedViews {
  views: RumSavedView[];
}

/** What is kept, as far as it is a list of views; anything else is nothing kept. */
export function parseSavedViews(raw: unknown): RumSavedViews | null {
  const views = (raw as { views?: unknown } | null)?.views;

  if (!Array.isArray(views)) return null;

  return {
    views: views.filter(
      (view): view is RumSavedView =>
        typeof view?.id === 'string' && typeof view?.name === 'string' && typeof view?.search === 'string',
    ),
  };
}

/**
 * The address a view is kept by: what shows it, without what depends on the moment. A range that
 * moves stays with it ("the last hour"); one fixed to moments is left to whoever opens it.
 */
export function viewSearchOf(share: URLSearchParams, time: string | null): string {
  const params = new URLSearchParams(share);

  if (time?.startsWith('now')) params.set('time', time);

  params.sort();

  return params.toString();
}

function nextId(views: readonly RumSavedView[]): string {
  const used = new Set(views.map((view) => view.id));
  let number = views.length + 1;

  while (used.has(`view-${number}`)) number += 1;

  return `view-${number}`;
}

/** The list with one more view, or with the view of that name kept by the new address. */
export function withSavedView(views: readonly RumSavedView[], name: string, search: string): RumSavedView[] {
  const trimmed = name.trim();
  const same = views.find((view) => view.name.toLowerCase() === trimmed.toLowerCase());

  if (same) return views.map((view) => (view === same ? { ...view, name: trimmed, search } : view));

  return [...views, { id: nextId(views), name: trimmed, search }];
}

export function withoutSavedView(views: readonly RumSavedView[], id: string): RumSavedView[] {
  return views.filter((view) => view.id !== id);
}

/** Whether a view can be kept under that name: there is room, and the address is not too long. */
export function canSaveView(views: readonly RumSavedView[], name: string, search: string): boolean {
  const trimmed = name.trim();
  const replaces = views.some((view) => view.name.toLowerCase() === trimmed.toLowerCase());

  return (
    trimmed !== '' &&
    trimmed.length <= MAX_VIEW_NAME &&
    search.length <= MAX_VIEW_SEARCH &&
    (replaces || views.length < MAX_SAVED_VIEWS)
  );
}
