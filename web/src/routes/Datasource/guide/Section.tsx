import { Check } from 'lucide-react';
import type * as React from 'react';

import { cn } from '@/shell/lib/cn';

/** `done`: nothing left to do here. `current`: where the reader is. `attention`: it was tried and needs a look. */
export type GuideSectionState = 'done' | 'current' | 'upcoming' | 'attention';

/**
 * One step of a guide, hung on the line that joins it to the next: a marker
 * that says how far along it is, the step's name, and what there is to do.
 * The steps are numbered once, here; nothing inside a step is numbered again.
 */
export function GuideSection({
  number,
  state,
  title,
  description,
  children,
}: {
  number: number;
  state: GuideSectionState;
  title: string;
  description?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <li
      data-guide-section={number}
      data-state={state}
      // The marker column is as wide as the guide's icon, so every title starts on one edge.
      className="group/section relative grid grid-cols-[40px_minmax(0,1fr)] gap-x-[14px] pb-[32px] last:pb-0"
    >
      <span
        aria-hidden
        className="absolute bottom-[4px] left-[19.5px] top-[36px] w-px bg-bd-0 group-last/section:hidden"
      />
      <span
        aria-hidden
        className={cn(
          'grid h-[28px] w-[28px] place-items-center justify-self-center rounded-full font-mono text-xs font-bold transition-colors duration-fast',
          state === 'done' && 'bg-[var(--green-tint)] text-[var(--green-ink)]',
          state === 'current' && 'bg-indigo-dim text-indigo-soft',
          state === 'attention' && 'bg-[var(--yellow-tint)] text-[var(--yellow-ink)]',
          state === 'upcoming' && 'bg-bg-2 text-tx-3',
        )}
      >
        {state === 'done' ? <Check className="h-3.5 w-3.5 stroke-[3]" /> : number}
      </span>
      <div className="min-w-0">
        <h3 className="m-0 flex min-h-[28px] items-center font-sans text-lg font-strong text-tx-0">
          {title}
        </h3>
        {description && (
          <p className="m-0 mt-[2px] max-w-[680px] font-sans text-sm leading-relaxed text-tx-2">
            {description}
          </p>
        )}
        <div className="mt-[14px] min-w-0">{children}</div>
      </div>
    </li>
  );
}
