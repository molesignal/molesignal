import type { LucideIcon } from 'lucide-react';
import * as React from 'react';
import { Link, type To, useLocation } from 'react-router-dom';

import { cn } from '@/shell/lib/cn';

import { pickActiveTab } from './matchTab';
import { useTabStrip } from './useTabStrip';
import './tabs.css';

export interface ModuleTabItem {
  key: string;
  label: string;
  /** Where the tab goes. Tabs of a strip that switches content in the page have none. */
  to?: To | undefined;
  /** Match only the exact path instead of the path and everything below it. */
  end?: boolean | undefined;
  /** Overrides the route match, for a tab that owns several routes. */
  active?: boolean | undefined;
  icon?: LucideIcon | undefined;
  count?: number | string | undefined;
  /** Neighbouring tabs with different groups are set apart by a divider. */
  group?: string | undefined;
  testId?: string | undefined;
}

type DataAttributes = Readonly<Record<`data-${string}`, string>>;

interface StripProps {
  /** The name of the whole strip, for assistive technology. */
  label: string;
  items: readonly ModuleTabItem[];
  /** Pushed to the far end of the strip, e.g. Settings. */
  trailing?: readonly ModuleTabItem[] | undefined;
  /**
   * `surface`: a card of its own under the page header.
   * `inline`: the header row of a card that already exists.
   */
  variant?: 'surface' | 'inline' | undefined;
  /** A hairline under an `inline` strip, for one that sits on the bare canvas. */
  divider?: boolean | undefined;
  /** A second row in the same card, for the tabs one level down. */
  children?: React.ReactNode;
  className?: string | undefined;
  /** `data-*` hooks on the outermost element. */
  dataAttributes?: DataAttributes | undefined;
}

export interface ModuleTabsProps extends StripProps {
  /** Selects in-page tabs. Without it the tabs are links and the route decides. */
  value?: string | undefined;
  onValueChange?: ((key: string) => void) | undefined;
  /** Gives each in-page tab the id `${tabIdPrefix}-${key}`, for a panel's `aria-labelledby`. */
  tabIdPrefix?: string | undefined;
}

// How far the indicator stops short of the tab's edges, so it sits under the
// label and not under the whole hover area.
const INDICATOR_INSET = 6;

const TAB_CLASS = cn(
  'group/tab relative inline-flex h-[40px] shrink-0 items-center px-[10px] font-sans text-md outline-none [@media(pointer:coarse)]:h-[44px]',
  // The hover and focus fill is inset from the strip, so it never touches the
  // indicator or the edge of the card.
  "before:pointer-events-none before:absolute before:inset-x-0 before:inset-y-[6px] before:rounded-md before:bg-transparent before:transition-colors before:duration-fast before:content-['']",
  'hover:before:bg-bg-2 active:before:bg-bg-3 focus-visible:before:bg-bg-3',
);

/**
 * The second level of navigation: where in a module you are. A strip of tabs
 * with one indicator that glides to the active tab, a divider between groups of
 * related tabs, and a fade at whichever edge has more tabs beyond it.
 */
export function ModuleTabs({ value, onValueChange, tabIdPrefix, ...strip }: ModuleTabsProps) {
  if (value !== undefined && onValueChange) {
    return (
      <ValueTabs
        {...strip}
        value={value}
        onValueChange={onValueChange}
        tabIdPrefix={tabIdPrefix}
      />
    );
  }
  return <RouteTabs {...strip} />;
}

function RouteTabs(props: StripProps) {
  const { pathname } = useLocation();
  const all = [...props.items, ...(props.trailing ?? [])];
  return <Strip {...props} activeKey={pickActiveTab(all, pathname)} mode="link" />;
}

function ValueTabs({
  value,
  onValueChange,
  tabIdPrefix,
  ...props
}: StripProps & {
  value: string;
  onValueChange: (key: string) => void;
  tabIdPrefix?: string | undefined;
}) {
  return (
    <Strip
      {...props}
      activeKey={value}
      mode="value"
      onSelect={onValueChange}
      tabIdPrefix={tabIdPrefix}
    />
  );
}

function Strip({
  label,
  items,
  trailing,
  variant = 'surface',
  divider = false,
  children,
  className,
  dataAttributes,
  activeKey,
  mode,
  onSelect,
  tabIdPrefix,
}: StripProps & {
  activeKey: string | undefined;
  mode: 'link' | 'value';
  onSelect?: ((key: string) => void) | undefined;
  tabIdPrefix?: string | undefined;
}) {
  const { scrollerRef, contentRef, box, animated, fadeStyle, onKeyDown } = useTabStrip(
    activeKey,
    { activateOnArrow: mode === 'value' },
  );
  const tabbable = activeKey ?? items[0]?.key;

  const renderTab = (item: ModuleTabItem) => (
    <Tab
      key={item.key}
      item={item}
      active={item.key === activeKey}
      mode={mode}
      tabbable={item.key === tabbable}
      onSelect={onSelect}
      id={tabIdPrefix ? `${tabIdPrefix}-${item.key}` : undefined}
    />
  );

  const row: React.ReactNode[] = [];
  items.forEach((item, index) => {
    const previous = items[index - 1];
    if (previous?.group !== undefined && item.group !== undefined && previous.group !== item.group) {
      row.push(
        <span
          key={`divider-${item.key}`}
          aria-hidden="true"
          data-tab-divider
          className="mx-[6px] my-auto h-[14px] w-px shrink-0 bg-bd-1"
        />,
      );
    }
    row.push(renderTab(item));
  });

  const strip = (
    <div
      ref={scrollerRef}
      role={mode === 'value' ? 'tablist' : undefined}
      aria-label={mode === 'value' ? label : undefined}
      className="tabs-scroll"
      style={fadeStyle}
      onKeyDown={onKeyDown}
    >
      <div ref={contentRef} className="relative flex min-w-full items-stretch gap-[2px] px-[10px]">
        {row}
        {trailing && trailing.length > 0 && (
          <div className="ml-auto flex items-stretch gap-[2px] pl-[10px]">
            {trailing.map(renderTab)}
          </div>
        )}
        <span
          aria-hidden="true"
          data-tab-indicator
          className={cn(
            'pointer-events-none absolute bottom-0 left-0 h-[2px] rounded-full bg-indigo',
            animated && 'transition-[transform,width] duration-[280ms] ease-[cubic-bezier(0.32,0.72,0,1)]',
            box ? 'opacity-100' : 'opacity-0',
          )}
          style={
            box
              ? {
                  width: Math.max(0, box.width - INDICATOR_INSET * 2),
                  transform: `translateX(${box.left + INDICATOR_INSET}px)`,
                }
              : undefined
          }
        />
      </div>
    </div>
  );

  const named = mode === 'value' ? (
    strip
  ) : (
    <nav aria-label={label} className="min-w-0">
      {strip}
    </nav>
  );
  const body = (
    <>
      {variant === 'inline' && divider ? <div className="border-b border-bd-0">{named}</div> : named}
      {children}
    </>
  );

  if (variant === 'inline') {
    return (
      <div className={className} {...dataAttributes}>
        {body}
      </div>
    );
  }
  return (
    <div
      data-module-tabs="surface"
      className={cn(
        'mx-[20px] mb-[12px] overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]',
        className,
      )}
      {...dataAttributes}
    >
      {body}
    </div>
  );
}

function Tab({
  item,
  active,
  mode,
  tabbable,
  onSelect,
  id,
}: {
  item: ModuleTabItem;
  active: boolean;
  mode: 'link' | 'value';
  tabbable: boolean;
  onSelect?: ((key: string) => void) | undefined;
  id?: string | undefined;
}) {
  const Icon = item.icon;
  const className = cn(
    TAB_CLASS,
    'transition-colors duration-fast',
    active ? 'text-tx-0' : 'text-tx-2 hover:text-tx-0 focus-visible:text-tx-0',
  );
  const content = (
    <span className="relative inline-flex items-center gap-[7px]">
      {Icon && (
        <Icon
          aria-hidden="true"
          strokeWidth={1.75}
          className={cn(
            'h-[15px] w-[15px] shrink-0 transition-colors duration-fast',
            active ? 'text-indigo' : 'text-tx-3 group-hover/tab:text-tx-2',
          )}
        />
      )}
      {/* The bold label is laid out invisibly under the shown one, so the tab
          keeps the width it has when active and nothing shifts on selection. */}
      <span className="inline-grid">
        <span className={cn('col-start-1 row-start-1', active ? 'font-strong' : 'font-body')}>
          {item.label}
        </span>
        <span aria-hidden="true" className="invisible col-start-1 row-start-1 font-strong">
          {item.label}
        </span>
      </span>
      {item.count !== undefined && (
        <span
          className={cn(
            'type-micro grid h-[18px] min-w-[18px] place-items-center rounded-full px-[5px] tabular-nums transition-colors duration-fast',
            active ? 'bg-indigo-dim text-indigo-soft' : 'bg-bg-2 text-tx-2',
          )}
        >
          {item.count}
        </span>
      )}
    </span>
  );

  if (mode === 'value') {
    return (
      <button
        type="button"
        role="tab"
        id={id}
        data-tab
        data-tab-active={active}
        data-testid={item.testId}
        aria-selected={active}
        tabIndex={tabbable ? 0 : -1}
        onClick={() => onSelect?.(item.key)}
        className={className}
      >
        {content}
      </button>
    );
  }
  if (!item.to) return null;
  return (
    <Link
      to={item.to}
      data-tab
      data-tab-active={active}
      data-testid={item.testId}
      aria-current={active ? 'page' : undefined}
      className={className}
    >
      {content}
    </Link>
  );
}
