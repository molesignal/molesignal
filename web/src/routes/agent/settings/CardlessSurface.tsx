import type { ReactNode } from 'react';

import { cn } from '@/shell/lib/cn';

export function AgentSettingsSection({
  title,
  description,
  action,
  children,
  bodyClassName,
  headerDivider = true,
}: {
  title: ReactNode;
  description: ReactNode;
  action?: ReactNode;
  children: ReactNode;
  bodyClassName?: string | undefined;
  headerDivider?: boolean | undefined;
}) {
  return (
    <section
      data-agent-settings-section
      className="min-w-0 bg-transparent"
    >
      <header
        className={cn(
          'flex min-h-[60px] flex-wrap items-center gap-4 py-3',
          headerDivider && 'border-b border-bd-0',
        )}
      >
        <div className="min-w-0 flex-1">
          <h2 className="font-sans text-sm font-display-strong text-tx-0">
            {title}
          </h2>
          <p className="mt-1 font-sans text-xs leading-5 text-tx-2">
            {description}
          </p>
        </div>
        {action && <div className="ml-auto shrink-0">{action}</div>}
      </header>
      <div className={cn('pt-4', bodyClassName)}>{children}</div>
    </section>
  );
}

export function AgentProfileList({ children }: { children: ReactNode }) {
  return (
    <div data-agent-profile-list className="divide-y divide-bd-0">
      {children}
    </div>
  );
}

export function AgentProfileRow({ children }: { children: ReactNode }) {
  return (
    <article data-agent-profile-row className="min-w-0 py-4">
      {children}
    </article>
  );
}
