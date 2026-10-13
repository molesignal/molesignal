/**
 * Splits a formatted figure ("1.2 GiB", "12.3K", "46.9 req/s", "1.12%") into
 * number and unit, so the unit can be set smaller than the figure it qualifies.
 */
export function splitReadoutValue(formatted: string): { number: string; unit: string } {
  const match = /^(-?\d[\d.,]*)\s*(.*)$/.exec(formatted.trim());
  if (!match) return { number: formatted, unit: '' };
  return { number: match[1] ?? formatted, unit: match[2] ?? '' };
}

/** The number in a formatted figure, when it is one that can be counted up to. */
export function readoutNumber(number: string): { value: number; decimals: number; grouped: boolean } | null {
  if (!/^-?\d[\d,]*(?:\.\d+)?$/.test(number)) return null;
  const value = Number(number.replace(/,/g, ''));
  if (!Number.isFinite(value)) return null;
  return {
    value,
    decimals: number.includes('.') ? (number.split('.')[1]?.length ?? 0) : 0,
    grouped: number.includes(','),
  };
}
