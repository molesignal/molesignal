import type * as React from 'react';

/**
 * A row of a list that can run to thousands of rows. The browser leaves the
 * rows it has not scrolled to unlaid and unpainted until it does; they stay in
 * the page, so a search in the page finds them and the keyboard reaches them.
 *
 * It goes with a row component that is drawn again only when its own row
 * changes (`React.memo`): the two together keep a page of five thousand rows
 * as quick to open, scroll and select in as a page of a hundred.
 */
export const LONG_LIST_ROW_CLASS = '[content-visibility:auto]';

/**
 * The height the browser reserves for a row it has not drawn yet, so the
 * scrollbar is right from the start. It is the height of what is inside the
 * row: the row's own padding and border are added to it.
 */
export function longListRowStyle(contentHeightPx: number): React.CSSProperties {
  return { containIntrinsicBlockSize: `auto ${contentHeightPx}px` };
}
