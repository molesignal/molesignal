/** A count in the language of the page: 4,812, and 12.3K or 12.3万 from ten thousand. */
export function formatCount(value: number, language?: string): string {
  return new Intl.NumberFormat(language, {
    notation: value >= 10_000 ? 'compact' : 'standard',
    maximumFractionDigits: 1,
  }).format(value);
}

/** A share of a whole in whole percent: `38%`, and `<1%` for what is there but small. */
export function formatPercent(fraction: number): string {
  if (!Number.isFinite(fraction) || fraction <= 0) return '0%';
  if (fraction < 0.01) return '<1%';

  return `${Math.round(fraction * 100)}%`;
}
