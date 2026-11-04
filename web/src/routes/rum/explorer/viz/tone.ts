import { isAdditive, type RumMeasureChoice } from './spec';
import { type Grade, gradeOf, inUnit, VITALS } from '../../overview/model';

/** The word of a number in its colour: what is poor is in the strong weight, as the table of rows has it. */
export const GRADE_TEXT: Record<Grade, string> = {
  good: 'text-[var(--green-ink)]',
  needs_improvement: 'font-strong text-[var(--yellow-ink)]',
  poor: 'font-strong text-[var(--red-ink)]',
};

/** How a number of a vital grades, when the number drawn is one: a count or a sum of its field is not. */
export function vitalGrade(measure: RumMeasureChoice, value: number | string | null | undefined): Grade | null {
  if (typeof value !== 'number' || isAdditive(measure.fn)) return null;

  const vital = VITALS.find((candidate) => candidate.field === measure.field);

  return vital ? gradeOf(vital, inUnit(vital, value)) : null;
}
