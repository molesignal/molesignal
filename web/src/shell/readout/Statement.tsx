import type * as React from 'react';

import { cn } from '@/shell/lib/cn';

/** One cell of a statement: the surface and inset every readout beside the verdict shares. */
export const STATEMENT_CELL = 'bg-[var(--functional-surface)] px-[20px] py-[16px]';

const READOUT_COLUMNS = {
  1: '',
  2: 'sm:grid-cols-2',
  3: 'sm:grid-cols-3',
  4: 'sm:grid-cols-2 xl:grid-cols-4',
} as const;

type DataAttributes = Readonly<Record<`data-${string}`, string>>;

/**
 * The opening statement of a module page: the sentence a reader came for, and
 * the figures that sentence rests on.
 *
 * It is one surface. The verdict is on the left, in its own colour, which also
 * runs down the edge and pools in the corner it is read from; whatever shows
 * how the whole divides sits under it. The figures stand beside it as cells
 * parted by hairlines, so they read as one instrument and not as a row of
 * cards. A page has one of these, at its top, and nothing it says is said
 * again below it.
 */
export function Statement({
  label,
  tone,
  eyebrow,
  title,
  titleClassName,
  detail,
  children,
  readouts,
  readoutColumns = 3,
  dataAttributes,
  className,
}: {
  /** What the statement is of, for assistive technology. */
  label: string;
  /** The verdict's colour, as a CSS colour: `var(--red)`. */
  tone: string;
  eyebrow?: React.ReactNode;
  title: React.ReactNode;
  /** The verdict as coloured words: `text-red`. Without it the title is in the page's ink. */
  titleClassName?: string | undefined;
  detail?: React.ReactNode;
  /** Under the verdict: how the whole divides, and the way forward. */
  children?: React.ReactNode;
  /** `Readout` cells with `STATEMENT_CELL`, beside the verdict. */
  readouts?: React.ReactNode;
  readoutColumns?: keyof typeof READOUT_COLUMNS;
  dataAttributes?: DataAttributes | undefined;
  className?: string | undefined;
}) {
  return (
    <section
      aria-label={label}
      data-statement
      style={{ '--tone': tone } as React.CSSProperties}
      className={cn(
        'ms-rise grid overflow-hidden rounded-md bg-bd-0 [box-shadow:inset_3px_0_0_var(--tone),var(--shadow-functional-surface)]',
        readouts && 'xl:grid-cols-[minmax(320px,0.9fr)_minmax(0,2fr)] xl:gap-px',
        className,
      )}
      {...dataAttributes}
    >
      <div
        // The verdict's own colour pools in the corner it is read from.
        style={{
          backgroundImage:
            'radial-gradient(120% 170% at 0% 0%, color-mix(in srgb, var(--tone) 9%, transparent), transparent 62%)',
        }}
        className={cn('min-w-0', STATEMENT_CELL)}
      >
        {eyebrow && <div className="type-eyebrow font-sans text-tx-2">{eyebrow}</div>}
        <h2
          className={cn(
            'type-display text-balance font-sans font-display tracking-normal',
            eyebrow && 'mt-[6px]',
            titleClassName ?? 'text-tx-0',
          )}
        >
          {title}
        </h2>
        {detail && <p className="type-data mt-1 text-pretty text-tx-1">{detail}</p>}
        {children}
      </div>

      {readouts && (
        <div
          className={cn(
            'grid min-w-0 gap-px border-t border-bd-0 xl:border-t-0',
            READOUT_COLUMNS[readoutColumns],
          )}
        >
          {readouts}
        </div>
      )}
    </section>
  );
}
