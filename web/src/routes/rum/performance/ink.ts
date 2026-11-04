import type { Grade } from '../overview/model';

/** Each grade in the ink of what it says, as the overview and its pages table read them. */
export const GRADE_TEXT: Record<Grade, string> = {
  good: 'text-[var(--green-ink)]',
  needs_improvement: 'text-[var(--yellow-ink)]',
  poor: 'text-[var(--red-ink)]',
};

/** Each grade in the fill of a bar. */
export const GRADE_FILL: Record<Grade, string> = {
  good: 'bg-[var(--green)]',
  needs_improvement: 'bg-[var(--yellow)]',
  poor: 'bg-[var(--red)]',
};
