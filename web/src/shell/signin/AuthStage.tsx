import type * as React from 'react';

import { cn } from '@/shell/lib/cn';

import './signin.css';

/**
 * The page every authentication screen sits on: a full-height scroller with a
 * slow wash of the brand colours and a dot grid behind one centred column.
 * `h-[100dvh]` follows the visible viewport on phones whose address bar
 * collapses; a short window scrolls instead of clipping the form.
 */
export function AuthStage({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string | undefined;
}) {
  return (
    <div
      data-testid="auth-stage"
      className="ms-auth-stage h-[100dvh] w-full overflow-y-auto bg-bg-0"
    >
      <div aria-hidden className="ms-auth-wash" />
      <main
        className={cn(
          'relative mx-auto flex min-h-full w-full max-w-[440px] flex-col items-center justify-center px-4 py-10 sm:px-6 sm:py-14',
          className,
        )}
      >
        {children}
      </main>
    </div>
  );
}
