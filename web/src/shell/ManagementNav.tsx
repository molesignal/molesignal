import { ChevronDown, Search } from 'lucide-react';
import * as React from 'react';
import { NavLink } from 'react-router-dom';

import { uiLabelClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';

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
  className,
}: ManagementNavProps) {
  const [query, setQuery] = React.useState('');
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
    <nav
      aria-label={ariaLabel}
      className={cn(
        'sticky top-4 flex max-h-[calc(100vh-var(--topbar-h)-80px)] min-h-0 flex-col border-r border-bd-0 pr-4',
        className,
      )}
    >
      <div className="mb-3 pr-7">
        <label className="flex h-10 items-center gap-2 rounded-md border border-bd-1 bg-bg-1 px-2.5 lg:h-9">
          <Search className="h-3.5 w-3.5 shrink-0 text-tx-3" />
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={searchPlaceholder}
            aria-label={searchAriaLabel}
            className="min-w-0 flex-1 bg-transparent font-sans text-base text-tx-0 placeholder:text-tx-3 focus:outline-none lg:text-xs"
          />
        </label>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {filteredGroups.map((group) => {
          const expanded = needle.length > 0 || expandedGroups.has(group.key);
          return (
            <div key={group.key} className="mb-1 last:mb-0">
              <button
                type="button"
                onClick={() => toggleGroup(group.key)}
                aria-expanded={expanded}
                aria-label={
                  expanded
                    ? collapseGroupLabel(group.label)
                    : expandGroupLabel(group.label)
                }
                className={cn(
                  'flex min-h-11 w-full items-center gap-2 rounded-md px-2.5 text-left hover:bg-bg-2 lg:min-h-8',
                  'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo',
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
              {expanded && (
                <div className="flex flex-col gap-0.5 py-0.5 pl-1.5">
                  {group.sections.map((section) => (
                    <NavLink
                      key={section.to}
                      to={section.to}
                      className={({ isActive }) =>
                        cn(
                          'relative flex min-h-11 items-center rounded-md px-2.5 font-sans text-sm font-strong text-tx-1 hover:bg-bg-3 hover:text-tx-0 lg:min-h-8 lg:text-xs',
                          'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo',
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
        {filteredGroups.length === 0 && (
          <div className="px-2.5 py-6 text-center font-sans text-xs leading-relaxed text-tx-3">
            {noResultsLabel}
          </div>
        )}
      </div>
    </nav>
  );
}
