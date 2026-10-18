/**
 * An optional non-negative number typed into a field: empty means "not set",
 * anything else must be a finite number of zero or more.
 */
export function optionalAmount(value: string): number | null | 'invalid' {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : 'invalid';
}
