/**
 * The height of an explorer's foot: the row of the results' pages and, beside it, the count under
 * the field list, so the two stand on one line. On a desktop it is as tall as a row of the table
 * over it (32px); where the pages' buttons are touched, it keeps their 40px.
 */
export const EXPLORER_FOOT_HEIGHT = 'h-11 lg:h-9';

/** The row of the results' pages at an explorer's foot: on the results' own ground, no line over it. */
export const EXPLORER_PAGINATION_CLASS = `border-t-0 bg-transparent ${EXPLORER_FOOT_HEIGHT}`;
