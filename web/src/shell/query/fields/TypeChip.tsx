import { cn } from '@/shell/lib/cn';

/** The kinds of value a field can hold, by the colour each is written in wherever a value is shown. */
export type FieldTypeTone = 'green' | 'orange' | 'blue' | 'purple' | 'neutral';

const CHIP: Record<FieldTypeTone, string> = {
  green: 'bg-[var(--green-tint)] text-[var(--green-ink)]',
  orange: 'bg-[var(--orange-tint)] text-[var(--orange-ink)]',
  blue: 'bg-[var(--blue-tint)] text-[var(--blue-ink)]',
  purple: 'bg-[var(--purple-tint)] text-[var(--purple-ink)]',
  neutral: 'bg-bg-3 text-tx-2',
};
const LETTER: Record<FieldTypeTone, string> = {
  green: 'text-[var(--green-ink)]',
  orange: 'text-[var(--orange-ink)]',
  blue: 'text-[var(--blue-ink)]',
  purple: 'text-[var(--purple-ink)]',
  neutral: 'text-tx-2',
};

/**
 * What kind of value a field holds, before its name: a letter on a tile of its
 * colour. A field listed inside another has the letter alone, so the tiles
 * mark the fields of the list itself and the eye can run down them.
 */
export function FieldTypeChip({
  glyph,
  tone,
  nested = false,
}: {
  glyph: string;
  tone: FieldTypeTone;
  nested?: boolean;
}) {
  return (
    <span
      aria-hidden="true"
      data-field-type={tone}
      className={cn(
        'type-micro grid h-[18px] w-[18px] shrink-0 place-items-center rounded-[4px] font-sans font-bold leading-none',
        nested ? LETTER[tone] : CHIP[tone],
      )}
    >
      {glyph}
    </span>
  );
}
