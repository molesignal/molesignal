/** Mole Agent settings surface primitives. */
import type { ReactNode } from 'react';

import { cn } from '@/shell/lib/cn';

/**
 * One section of the settings. The row of sections in the navigation names it,
 * so it opens with a line on what it holds and the one thing that can be added,
 * then its content, on the content surface itself: as a tinted panel with a
 * title of its own it was the third thing called by the section's name.
 */
export function AgentSettingsSection({
  title,
  description,
  action,
  children,
  bodyClassName,
}: {
  title: ReactNode;
  description: ReactNode;
  action?: ReactNode;
  children: ReactNode;
  bodyClassName?: string | undefined;
}) {
  return (
    <section data-agent-settings-section className="min-w-0">
      <header className="flex min-h-[32px] flex-wrap items-center gap-x-4 gap-y-2 pb-[10px]">
        <h2 className="sr-only">{title}</h2>
        <p className="min-w-0 flex-1 font-sans text-xs leading-5 text-tx-2">{description}</p>
        {action && <div className="ml-auto shrink-0">{action}</div>}
      </header>
      <div className={cn(bodyClassName)}>{children}</div>
    </section>
  );
}

export function AgentProfileList({ children }: { children: ReactNode }) {
  return (
    <div data-agent-profile-list className="space-y-2">
      {children}
    </div>
  );
}

export function AgentProfileRow({ children }: { children: ReactNode }) {
  return (
    // A tinted row on the white surface: the tint is what sets one profile from the next.
    <article data-agent-profile-row className="min-w-0 rounded-md bg-[var(--control-surface)] p-4">
      {children}
    </article>
  );
}
