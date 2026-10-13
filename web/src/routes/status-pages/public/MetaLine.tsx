import * as React from 'react';

import { cn } from '@/shell/lib/cn';

/**
 * A few facts about one thing, set on a line with a dot between them.
 *
 * The line wraps on a narrow screen, and a dot must never be what a line ends
 * or begins with. So each fact carries its dot in front of it, the row is
 * pulled left by the width of one, and the frame clips what sticks out: the
 * dot of whichever fact begins a line.
 */
export function MetaLine({ children, className }: { children: React.ReactNode; className?: string }) {
  const facts = React.Children.toArray(children);

  return (
    <span className={cn('block overflow-hidden', className)}>
      <span className="-ml-[1.125rem] flex flex-wrap gap-y-0.5">
        {facts.map((fact, index) => (
          <span
            key={index}
            className="flex min-w-0 before:w-[1.125rem] before:shrink-0 before:text-center before:text-tx-4 before:content-['·']"
          >
            <span className="min-w-0">{fact}</span>
          </span>
        ))}
      </span>
    </span>
  );
}
