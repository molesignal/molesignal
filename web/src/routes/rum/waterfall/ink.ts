/**
 * The ink a kind of resource is written in. Scripts and documents run, styles shape the page, images
 * and fonts are static, and the calls an application makes are its own: a colour for each, none of them
 * the orange and red that are kept for what went wrong.
 */
const TYPE_INK: Record<string, string> = {
  document: 'text-[var(--purple-ink)]',
  js: 'text-[var(--purple-ink)]',
  css: 'text-[var(--blue-ink)]',
  image: 'text-[var(--green-ink)]',
  media: 'text-[var(--green-ink)]',
  font: 'text-tx-2',
  fetch: 'text-[var(--indigo-ink)]',
  xhr: 'text-[var(--indigo-ink)]',
  beacon: 'text-[var(--indigo-ink)]',
};

export const typeInk = (type: string): string => TYPE_INK[type] ?? 'text-tx-2';

/** A request in the ink of how it ended: the answers that went wrong are strong, in orange or red. */
export function statusInk(status: number | null): string {
  if (status === null) return 'text-tx-2';
  if (status === 0 || status >= 500) return 'font-strong text-[var(--red-ink)]';
  if (status >= 400) return 'font-strong text-[var(--orange-ink)]';
  if (status >= 300) return 'text-[var(--yellow-ink)]';

  return 'text-[var(--green-ink)]';
}
