import { PanelLeftClose, PanelLeftOpen } from 'lucide-react';
import type * as React from 'react';

import { cn } from '@/shell/lib/cn';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

/** How a page's menu of sections folds away, where it can. */
export interface SectionsFolding {
  collapsed: boolean;
  /** The page holds the folded state itself: a folded menu leaves no strip beside the content. */
  fullyHidden: boolean;
  /** The menu draws its own control to fold beside its search field. */
  ownsCollapse: boolean;
  onToggle: () => void;
  expandLabel: string;
  collapseLabel: string;
}

/**
 * The menu of a page's sections, in a column of its own beside the content: on a surface page a
 * card of its own. A page that lets it fold (Settings, Access) passes `folding`; a list of a
 * module's pages (Alerts) stays open.
 */
export function SectionsAside({
  sections,
  appearance,
  folding,
}: {
  sections: React.ReactNode;
  appearance: 'default' | 'surface';
  folding?: SectionsFolding | undefined;
}) {
  if (folding?.collapsed && !folding.fullyHidden) {
    // Collapsed: a thin strip with just an expand affordance.
    return (
      <aside>
        <button
          type="button"
          onClick={folding.onToggle}
          aria-label={folding.expandLabel}
          title={folding.expandLabel}
          className={cn(
            'sticky top-6 hidden h-9 w-9 place-items-center rounded-md text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 lg:grid',
            appearance === 'surface'
              ? 'border-0 bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]'
              : 'border border-bd-0 bg-bg-1',
          )}
        >
          <PanelLeftOpen className="h-4 w-4" />
        </button>
        <div className="lg:hidden">{sections}</div>
      </aside>
    );
  }

  return (
    <aside className={cn('relative min-w-0', folding?.collapsed && folding.fullyHidden && 'lg:hidden')}>
      {folding && !folding.fullyHidden && !folding.ownsCollapse && (
        <button
          type="button"
          onClick={folding.onToggle}
          aria-label={folding.collapseLabel}
          title={folding.collapseLabel}
          className="absolute right-2 top-2 z-10 hidden h-8 w-8 place-items-center rounded-md text-tx-3 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 lg:grid"
        >
          <PanelLeftClose className="h-3.5 w-3.5" />
        </button>
      )}
      <div
        className={cn(
          appearance === 'surface' &&
            'rounded-md bg-[var(--functional-surface)] p-[12px] [box-shadow:var(--shadow-functional-surface)]',
        )}
      >
        {sections}
      </div>
    </aside>
  );
}

/**
 * The way back to a menu the page folded away: a faint button at the left edge of the content,
 * which stays in view as the content scrolls. The content around it is `relative`.
 */
export function SectionsExpand({ label, onExpand }: { label: string; onExpand: () => void }) {
  return (
    <div className="absolute -left-5 top-0 z-10 hidden h-full lg:block">
      <Tooltip>
        <TooltipTrigger asChild>
          <button
            type="button"
            onClick={onExpand}
            aria-label={label}
            className="sticky top-6 grid h-8 w-8 place-items-center rounded-md bg-bg-0 text-tx-3 opacity-50 transition-colors hover:bg-bg-2 hover:text-tx-0 hover:opacity-100 focus-visible:bg-bg-2 focus-visible:text-tx-0 focus-visible:opacity-100"
          >
            <PanelLeftOpen className="h-3.5 w-3.5" />
          </button>
        </TooltipTrigger>
        <TooltipContent side="right">{label}</TooltipContent>
      </Tooltip>
    </div>
  );
}
