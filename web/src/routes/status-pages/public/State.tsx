import { TriangleAlert } from 'lucide-react';
import type { ReactNode } from 'react';

import { cn } from '@/shell/lib/cn';

import { PublicStatusCredit } from './Credit';

/**
 * What a public status view shows when there is no page to show yet, or at
 * all: it is loading, or the address leads nowhere. One frame for the status
 * page, its history and its incidents, so the three cannot drift apart.
 */
export function PublicStatusState({
  title,
  credit,
  children,
}: {
  /** Present when something went wrong; a page that is only loading has none. */
  title?: string;
  /** What serves the page: said even when there is no page to show. */
  credit: string;
  children: ReactNode;
}) {
  return (
    <main
      data-theme="light"
      className="flex h-full min-h-screen flex-col overflow-y-auto bg-bg-0 px-6 text-center"
    >
      <div className="grid flex-1 place-items-center py-10">
        <div className="max-w-md">
          {title && (
            <>
              <TriangleAlert aria-hidden className="mx-auto h-9 w-9 text-tx-3" strokeWidth={1.5} />
              <h1 className="mt-5 text-2xl font-display-strong text-tx-0">{title}</h1>
            </>
          )}
          <p className={cn('text-sm leading-relaxed text-tx-2', title && 'mt-2')}>{children}</p>
        </div>
      </div>
      <footer className="flex shrink-0 justify-center pb-8">
        <PublicStatusCredit label={credit} />
      </footer>
    </main>
  );
}
