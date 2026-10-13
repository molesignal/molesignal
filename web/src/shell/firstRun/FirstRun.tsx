import type * as React from 'react';

import { cn } from '@/shell/lib/cn';

export interface FirstRunStep {
  title: React.ReactNode;
  description?: React.ReactNode;
}

/**
 * What a module shows before it has any data of its own.
 *
 * An empty table says "nothing here"; this says what the page will show and
 * how to get there. The words and the next step are on the left, a drawing of
 * what the module does with data is on the right, and the path from here to a
 * working page runs along the bottom. It is one surface, so it replaces the
 * page's content instead of sitting in a card above an empty canvas.
 */
export function FirstRun({
  eyebrow,
  title,
  description,
  actions,
  steps,
  illustration,
  tone = 'var(--indigo)',
  children,
  className,
  'data-testid': testId,
}: {
  eyebrow?: React.ReactNode;
  title: React.ReactNode;
  description?: React.ReactNode;
  /** The next step, and at most a second, quieter one. */
  actions?: React.ReactNode;
  steps?: readonly FirstRunStep[] | undefined;
  /** Decorative: a preview of what the module draws once data arrives. */
  illustration?: React.ReactNode;
  /** The colour the drawing's panel is tinted with: the module's own. */
  tone?: string | undefined;
  /** Anything between the description and the actions (a choice of platforms). */
  children?: React.ReactNode;
  className?: string | undefined;
  'data-testid'?: string | undefined;
}) {
  return (
    <section
      data-first-run
      data-testid={testId}
      className={cn(
        'ms-rise overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]',
        className,
      )}
    >
      <div
        className={cn(
          'grid gap-x-[40px] gap-y-[28px] px-[24px] py-[28px] lg:px-[36px] lg:py-[36px]',
          illustration && 'lg:grid-cols-[minmax(0,1fr)_minmax(0,440px)] lg:items-center',
        )}
      >
        <div className="min-w-0 max-w-[600px]">
          {eyebrow && <div className="type-eyebrow font-sans text-indigo-soft">{eyebrow}</div>}
          <h2
            className={cn(
              'type-display text-balance font-sans font-display tracking-[-0.02em] text-tx-0',
              eyebrow && 'mt-[10px]',
            )}
          >
            {title}
          </h2>
          {description && (
            <p className="mt-[10px] text-pretty font-sans text-sm leading-relaxed text-tx-2">
              {description}
            </p>
          )}
          {children}
          {actions && (
            <div className="mt-[22px] flex flex-wrap items-center gap-2">{actions}</div>
          )}
        </div>
        {illustration && (
          <div
            aria-hidden="true"
            data-first-run-art
            style={
              {
                '--tone': tone,
                backgroundImage:
                  'radial-gradient(120% 140% at 100% 0%, color-mix(in srgb, var(--tone) 10%, transparent), transparent 62%)',
              } as React.CSSProperties
            }
            className="hidden min-h-[212px] items-center justify-center rounded-lg bg-[var(--well-surface)] p-[20px] lg:flex"
          >
            {illustration}
          </div>
        )}
      </div>

      {steps && steps.length > 0 && (
        <ol
          className="grid gap-px border-t border-bd-0 bg-bd-0"
          style={{ gridTemplateColumns: `repeat(auto-fit, minmax(min(100%, 200px), 1fr))` }}
        >
          {steps.map((step, index) => (
            <li
              key={index}
              className="flex min-w-0 items-start gap-[12px] bg-[var(--functional-surface)] px-[24px] py-[16px] lg:px-[36px]"
            >
              <span className="type-micro mt-px grid h-[22px] w-[22px] shrink-0 place-items-center rounded-full bg-indigo-dim font-mono font-strong tabular-nums text-indigo-soft">
                {index + 1}
              </span>
              <span className="min-w-0">
                <span className="block font-sans text-sm font-strong text-tx-0">{step.title}</span>
                {step.description && (
                  <span className="mt-0.5 block text-pretty font-sans text-xs leading-relaxed text-tx-2">
                    {step.description}
                  </span>
                )}
              </span>
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}
