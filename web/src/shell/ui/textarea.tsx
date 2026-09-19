import * as React from 'react';

import { cn } from '@/shell/lib/cn';

const Textarea = React.forwardRef<
  HTMLTextAreaElement,
  React.ComponentProps<'textarea'>
>(({ className, ...props }, ref) => {
  return (
    <textarea
      data-ui="textarea-control"
      className={cn(
        'flex min-h-20 w-full rounded-md border-0 bg-[var(--control-surface)] px-3 py-2.5 text-sm transition-colors placeholder:text-muted-foreground hover:bg-bg-3 focus-visible:bg-bg-3 focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50',
        className,
      )}
      ref={ref}
      {...props}
    />
  );
});
Textarea.displayName = 'Textarea';

export { Textarea };
