import { SEGMENT_FIELDS, SEGMENT_USER_PREFIX } from '@/api/rum';
import type { RumSegment, RumSegmentCondition } from '@/api/rum';

import { MAX_FILTER_NAME, MAX_FILTER_VALUE, MAX_RETENTION_VALUES } from './retention';

/** Most segments an application has, and most conditions of one, as the server keeps them. */
export const MAX_SEGMENTS = 20;
export const MAX_SEGMENT_CONDITIONS = 5;

/** A condition on the country, with nothing yet to compare it with. */
export function newSegmentCondition(): RumSegmentCondition {
  return { field: 'geo.country_iso_code', op: 'is', values: [] };
}

/** A segment named so that no other has its name. */
export function newSegment(taken: readonly string[], named: (number: number) => string): RumSegment {
  const used = new Set(taken.map((name) => name.trim().toLowerCase()));
  let number = taken.length + 1;

  while (used.has(named(number).toLowerCase())) number += 1;

  return { id: '', name: named(number), conditions: [newSegmentCondition()] };
}

function validField(field: string): boolean {
  return (SEGMENT_FIELDS as readonly string[]).includes(field) || (field.startsWith(SEGMENT_USER_PREFIX) && field.length > SEGMENT_USER_PREFIX.length);
}

function validCondition(condition: RumSegmentCondition): boolean {
  return (
    validField(condition.field) &&
    condition.values.length >= 1 &&
    condition.values.length <= MAX_RETENTION_VALUES &&
    condition.values.every((value) => value.trim() !== '' && value.length <= MAX_FILTER_VALUE * 2)
  );
}

/** Whether segments are ones the server takes: each named apart from the others and saying something. */
export function validSegments(segments: readonly RumSegment[]): boolean {
  const names = new Set(segments.map((segment) => segment.name.trim().toLowerCase()));

  return (
    names.size === segments.length &&
    segments.length <= MAX_SEGMENTS &&
    segments.every(
      (segment) =>
        segment.name.trim() !== '' &&
        segment.name.length <= MAX_FILTER_NAME &&
        segment.conditions.length >= 1 &&
        segment.conditions.length <= MAX_SEGMENT_CONDITIONS &&
        segment.conditions.every(validCondition),
    )
  );
}

export function sameSegments(left: readonly RumSegment[], right: readonly RumSegment[]): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}
