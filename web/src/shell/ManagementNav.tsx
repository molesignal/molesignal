import { ChevronDown, PanelLeftClose, PanelLeftOpen, Search } from 'lucide-react';
import * as React from 'react';
import { NavLink, useNavigate } from 'react-router-dom';

import { uiLabelClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import {
  Sheet,
  SheetContent,
  SheetTitle,
  SheetTrigger,
} from '@/shell/ui/sheet';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

export interface ManagementNavSection {
  to: string;
  label: string;
  keywords?: readonly string[];
}

export interface ManagementNavGroup {
  key: string;
  label: string;
  sections: readonly ManagementNavSection[];
}

interface ManagementNavProps {
  ariaLabel: string;
  groups: readonly ManagementNavGroup[];
  currentPath: string;
  searchPlaceholder: string;
  searchAriaLabel: string;
  noResultsLabel: string;
  collapseGroupLabel: (group: string) => string;
  expandGroupLabel: (group: string) => string;
  collapsibleGroups?: boolean;
  onCollapse?: () => void;
  collapseNavigationLabel?: string;
  mobilePresentation?: 'select' | 'drawer';
  mobileTriggerLabel?: string;
  className?: string;
}

export function ManagementNav({
  ariaLabel,
  groups,
  currentPath,
  searchPlaceholder,
  searchAriaLabel,
  noResultsLabel,
  collapseGroupLabel,
  expandGroupLabel,
  collapsibleGroups = true,
  onCollapse,
  collapseNavigationLabel,
  mobilePresentation = 'select',
  mobileTriggerLabel = ariaLabel,
  className,
}: ManagementNavProps) {
  const navigate = useNavigate();
  const mobileNavId = React.useId();
  const [query, setQuery] = React.useState('');
  const [mobileOpen, setMobileOpen] = React.useState(false);
  const currentSection = groups
    .flatMap((group) => group.sections)
    .find(
      (section) =>
        currentPath === section.to || currentPath.startsWith(`${section.to}/`),
    );
  const currentGroup =
    groups.find((group) =>
      group.sections.some(
        (section) =>
          currentPath === section.to || currentPath.startsWith(`${section.to}/`),
      ),
    )?.key ?? groups[0]?.key;
  const [expandedGroups, setExpandedGroups] = React.useState<Set<string>>(
    () => new Set(currentGroup ? [currentGroup] : []),
  );

  React.useEffect(() => {
    if (!currentGroup) return;
    setExpandedGroups((current) => {
      if (current.has(currentGroup)) return current;
      return new Set([...current, currentGroup]);
    });
  }, [currentGroup]);

  const needle = query.trim().toLocaleLowerCase();
  const filteredGroups = groups
    .map((group) => ({
      ...group,
      sections: group.sections.filter((section) =>
        [section.label, ...(section.keywords ?? [])]
          .join(' ')
          .toLocaleLowerCase()
          .includes(needle),
      ),
    }))
    .filter((group) => group.sections.length > 0);

  const toggleGroup = (key: string) => {
    setExpandedGroups((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  return (
    <>
      {mobilePresentation === 'drawer' ? (
        <Sheet open={mobileOpen} onOpenChange={setMobileOpen}>
          <SheetTrigger asChild>
            <button
              type="button"
              aria-label={mobileTriggerLabel}
              className="flex h-11 w-full items-center gap-2 rounded-md border border-bd-0 bg-bg-1 px-3 font-sans text-base font-strong text-tx-1 hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 lg:hidden"
            >
              <PanelLeftOpen className="h-4 w-4 shrink-0 text-tx-3" />
              <span className="min-w-0 flex-1 truncate text-left">
                {mobileTriggerLabel}
              </span>
              <ChevronDown className="h-4 w-4 shrink-0 -rotate-90 text-tx-3" />
            </button>
          </SheetTrigger>
          <SheetContent
            side="left"
            className="bottom-0 top-topbar flex h-auto w-[min(320px,calc(100vw-16px))] max-w-none flex-col overflow-hidden border-bd-0 bg-bg-0 p-4 data-[state=open]:animate-none md:left-sidebar-collapsed"
          >
            <SheetTitle className="sr-only">{ariaLabel}</SheetTitle>
            <nav
              aria-label={ariaLabel}
              className="mt-6 flex min-h-0 flex-1 flex-col"
            >
              <ManagementNavContents
                query={query}
                onQueryChange={setQuery}
                searchPlaceholder={searchPlaceholder}
                searchAriaLabel={searchAriaLabel}
                groups={filteredGroups}
                needle={needle}
                expandedGroups={expandedGroups}
                collapsibleGroups={collapsibleGroups}
                collapseGroupLabel={collapseGroupLabel}
                expandGroupLabel={expandGroupLabel}
                onToggleGroup={toggleGroup}
                noResultsLabel={noResultsLabel}
                onNavigate={() => setMobileOpen(false)}
              />
            </nav>
          </SheetContent>
        </Sheet>
      ) : (
        <div className="relative lg:hidden">
          <label className="sr-only" htmlFor={mobileNavId}>
            {ariaLabel}
          </label>
          <select
            id={mobileNavId}
            value={currentSection?.to ?? groups[0]?.sections[0]?.to ?? ''}
            onChange={(event) => navigate(event.target.value)}
            className="h-11 w-full appearance-none rounded-md border border-bd-1 bg-bg-1 px-3 pr-10 font-sans text-base text-tx-0 focus:bg-bg-2"
          >
            {groups.map((group) => (
              <optgroup key={group.key} label={group.label}>
                {group.sections.map((section) => (
                  <option key={section.to} value={section.to}>
                    {section.label}
                  </option>
                ))}
              </optgroup>
            ))}
          </select>
          <ChevronDown
            aria-hidden
            className="pointer-events-none absolute right-3 top-1/2 h-4 w-4 -translate-y-1/2 text-tx-3"
          />
        </div>
      )}

      <nav
        aria-label={ariaLabel}
        className={cn(
          'group/settings-nav sticky top-4 hidden max-h-[calc(100vh-var(--topbar-h)-80px)] min-h-0 flex-col border-r border-bd-0 pr-4 lg:flex',
          className,
        )}
      >
        <ManagementNavContents
          query={query}
          onQueryChange={setQuery}
          searchPlaceholder={searchPlaceholder}
          searchAriaLabel={searchAriaLabel}
          groups={filteredGroups}
          needle={needle}
          expandedGroups={expandedGroups}
          collapsibleGroups={collapsibleGroups}
          collapseGroupLabel={collapseGroupLabel}
          expandGroupLabel={expandGroupLabel}
          onToggleGroup={toggleGroup}
          noResultsLabel={noResultsLabel}
          {...(onCollapse && collapseNavigationLabel
            ? { onCollapse, collapseNavigationLabel }
            : {})}
        />
      </nav>
    </>
  );
}

function ManagementNavContents({
  query,
  onQueryChange,
  searchPlaceholder,
  searchAriaLabel,
  groups,
  needle,
  expandedGroups,
  collapsibleGroups,
  collapseGroupLabel,
  expandGroupLabel,
  onToggleGroup,
  noResultsLabel,
  onNavigate,
  onCollapse,
  collapseNavigationLabel,
}: {
  query: string;
  onQueryChange: (query: string) => void;
  searchPlaceholder: string;
  searchAriaLabel: string;
  groups: readonly ManagementNavGroup[];
  needle: string;
  expandedGroups: ReadonlySet<string>;
  collapsibleGroups: boolean;
  collapseGroupLabel: (group: string) => string;
  expandGroupLabel: (group: string) => string;
  onToggleGroup: (key: string) => void;
  noResultsLabel: string;
  onNavigate?: () => void;
  onCollapse?: () => void;
  collapseNavigationLabel?: string;
}) {
  return (
    <>
      <div className="mb-3 flex items-center gap-1">
        <label className="flex h-10 min-w-0 flex-1 items-center gap-2 rounded-md border border-bd-1 bg-bg-1 px-2.5 lg:h-9">
          <Search className="h-3.5 w-3.5 shrink-0 text-tx-3" />
          <input
            value={query}
            onChange={(event) => onQueryChange(event.target.value)}
            placeholder={searchPlaceholder}
            aria-label={searchAriaLabel}
            className="min-w-0 flex-1 bg-transparent font-sans text-base text-tx-0 placeholder:text-tx-3 focus:outline-none lg:text-xs"
          />
        </label>
        {onCollapse && collapseNavigationLabel && (
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                onClick={onCollapse}
                aria-label={collapseNavigationLabel}
                className="grid h-9 w-8 shrink-0 place-items-center rounded-md text-tx-3 opacity-40 transition-colors hover:bg-bg-2 hover:text-tx-0 hover:opacity-100 focus-visible:bg-bg-2 focus-visible:text-tx-0 focus-visible:opacity-100 group-hover/settings-nav:opacity-100 group-focus-within/settings-nav:opacity-100"
              >
                <PanelLeftClose className="h-3.5 w-3.5" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              {collapseNavigationLabel}
            </TooltipContent>
          </Tooltip>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {groups.map((group) => {
          const expanded =
            !collapsibleGroups ||
            needle.length > 0 ||
            expandedGroups.has(group.key);
          return (
            <div key={group.key} className="mb-4 last:mb-0">
              {collapsibleGroups ? (
                <button
                  type="button"
                  onClick={() => onToggleGroup(group.key)}
                  aria-expanded={expanded}
                  aria-label={
                    expanded
                      ? collapseGroupLabel(group.label)
                      : expandGroupLabel(group.label)
                  }
                  className={cn(
                    'flex min-h-11 w-full items-center gap-2 rounded-md px-2.5 text-left hover:bg-bg-2 focus-visible:bg-bg-2 lg:min-h-8',
                    uiLabelClass,
                  )}
                >
                  <span className="min-w-0 flex-1 truncate">{group.label}</span>
                  <ChevronDown
                    className={cn(
                      'h-3.5 w-3.5 shrink-0 text-tx-3 transition-transform duration-fast',
                      !expanded && '-rotate-90',
                    )}
                  />
                </button>
              ) : (
                <div className={cn('flex h-8 items-center px-2.5', uiLabelClass)}>
                  {group.label}
                </div>
              )}
              {expanded && (
                <div className="flex flex-col gap-0.5 py-0.5 pl-1.5">
                  {group.sections.map((section) => (
                    <NavLink
                      key={section.to}
                      to={section.to}
                      onClick={onNavigate}
                      className={({ isActive }) =>
                        cn(
                          'relative flex min-h-11 items-center rounded-md px-2.5 font-sans text-sm font-strong text-tx-1 hover:bg-bg-3 hover:text-tx-0 lg:min-h-8 lg:text-xs',
                          'focus-visible:bg-bg-3 focus-visible:text-tx-0',
                          isActive &&
                            'bg-indigo-dim text-indigo-soft before:absolute before:-left-1.5 before:top-1/2 before:h-5 before:w-0.5 before:-translate-y-1/2 before:rounded-r before:bg-indigo',
                        )
                      }
                    >
                      {section.label}
                    </NavLink>
                  ))}
                </div>
              )}
            </div>
          );
        })}
        {groups.length === 0 && (
          <div className="px-2.5 py-6 text-center font-sans text-xs leading-relaxed text-tx-3">
            {noResultsLabel}
          </div>
        )}
      </div>
    </>
  );
}
