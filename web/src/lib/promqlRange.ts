/**
 * The step and row cap of a PromQL range query. `POST /query` reads PromQL
 * as a range when it is given a step and at the end of the range without
 * one; the row limit only caps the answer.
 */

const SECOND_MICROS = 1_000_000;

/** Steps a PromQL chart asks for when nobody chose a step: about one per pixel of a wide chart. */
export const PROMQL_AUTO_STEPS = 1_000;

/** The most steps a series may have; the server refuses a finer step. */
export const PROMQL_MAX_STEPS = 11_000;

/** Rows a PromQL range answer may hold before the server samples each series evenly. */
export const PROMQL_RANGE_ROWS = 50_000;

/**
 * The step of a range query from `startMicros` to `endMicros`: the chosen
 * one, or the range divided into `steps` parts, never finer than a second
 * nor than the server allows.
 */
export function promqlStepMicros(
  startMicros: number,
  endMicros: number,
  chosenMicros?: number | null,
  steps: number = PROMQL_AUTO_STEPS,
): number {
  const span = Math.max(endMicros - startMicros, 0);
  const finest = Math.max(SECOND_MICROS, Math.ceil(span / (PROMQL_MAX_STEPS - 1)));
  const wanted = chosenMicros && chosenMicros > 0 ? chosenMicros : Math.ceil(span / steps);

  return Math.max(finest, Math.round(wanted));
}
