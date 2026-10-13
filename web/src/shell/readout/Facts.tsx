import { cn } from '@/shell/lib/cn';

export interface Fact {
  key: string;
  label: string;
  value: string;
  /** A figure that is itself the news: more of it is bad, or it is a warning, or none of it is good. */
  tone?: 'danger' | 'warning' | 'good' | undefined;
  /** What the figure counts, for a pointer that rests on it. */
  hint?: string | undefined;
  /** A figure that narrows what stands under it when pressed. */
  onPress?: (() => void) | undefined;
  /** The figure the content is narrowed to. */
  active?: boolean | undefined;
}

/**
 * What a page counted, in one line: each figure after the name of what it
 * counts. A row of number cards takes a third of a screen to say as much, and
 * leaves the list it describes below the fold.
 *
 * On the canvas it is a surface of its own (`surface`); inside a surface it is
 * a row of it. A figure that filters is a button and looks like one only under
 * the pointer.
 */
export function Facts({
  label,
  facts,
  surface = false,
  className,
  ...rest
}: {
  label: string;
  facts: readonly Fact[];
  surface?: boolean | undefined;
  className?: string | undefined;
} & Readonly<Record<`data-${string}`, string>>) {
  return (
    <section
      data-facts
      aria-label={label}
      {...rest}
      className={cn(
        'flex min-w-0 flex-wrap items-baseline gap-x-[20px] gap-y-[4px] font-sans text-xs',
        surface &&
          'rounded-md bg-[var(--functional-surface)] px-[12px] py-[8px] [box-shadow:var(--shadow-functional-surface)]',
        className,
      )}
    >
      {facts.map((fact) => {
        const body = (
          <>
            <span className="text-tx-2">{fact.label}</span>
            <span
              className={cn(
                'text-sm font-strong tabular-nums',
                fact.tone === 'danger' && 'text-[var(--red-ink)]',
                fact.tone === 'warning' && 'text-[var(--yellow-ink)]',
                fact.tone === 'good' && 'text-[var(--green-ink)]',
                !fact.tone && 'text-tx-0',
              )}
            >
              {fact.value}
            </span>
          </>
        );

        return fact.onPress ? (
          <button
            key={fact.key}
            type="button"
            data-fact={fact.key}
            aria-pressed={fact.active === undefined ? undefined : fact.active}
            title={fact.hint}
            onClick={fact.onPress}
            className={cn(
              // The room it is pressed in reaches past its line without making the line taller.
              'mx-[-6px] my-[-3px] flex items-baseline gap-[6px] whitespace-nowrap rounded-md px-[6px] py-[3px] hover:bg-bg-2 focus-visible:bg-bg-2',
              fact.active && 'bg-[var(--floating-item-selected)]',
            )}
          >
            {body}
          </button>
        ) : (
          <span
            key={fact.key}
            data-fact={fact.key}
            title={fact.hint}
            className="flex items-baseline gap-[6px] whitespace-nowrap"
          >
            {body}
          </span>
        );
      })}
    </section>
  );
}
