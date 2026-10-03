import { Link, type To, useLocation } from 'react-router-dom';

import { cn } from '@/shell/lib/cn';

import { pickActiveTab } from './matchTab';
import { useTabStrip } from './useTabStrip';
import './tabs.css';

export interface ViewTabItem<T extends string = string> {
  key: T;
  label: string;
  count?: number | string | undefined;
  /** Where the view goes. Views that filter the page in place have none. */
  to?: To | undefined;
  end?: boolean | undefined;
  active?: boolean | undefined;
}

type DataAttributes = Readonly<Record<`data-${string}`, string>>;

interface ViewTabsProps<T extends string> {
  /** The name of the group, for assistive technology. */
  label: string;
  items: readonly ViewTabItem<T>[];
  /** Selects in-page views. Without it the views are links and the route decides. */
  value?: T | undefined;
  onValueChange?: ((key: T) => void) | undefined;
  className?: string | undefined;
  dataAttributes?: DataAttributes | undefined;
}

/**
 * The third level: which view of this page. Quieter than the module tabs above
 * it and unlike them, so the two levels never read as one: no underline, a soft
 * indigo pill that glides to the selected view, and no track that could be
 * mistaken for a field.
 */
export function ViewTabs<T extends string>({
  value,
  onValueChange,
  ...group
}: ViewTabsProps<T>) {
  if (value !== undefined && onValueChange) {
    return (
      <Group<T> {...group} activeKey={value} mode="value" onSelect={onValueChange} />
    );
  }
  return <RouteGroup<T> {...group} />;
}

function RouteGroup<T extends string>(props: Omit<ViewTabsProps<T>, 'value' | 'onValueChange'>) {
  const { pathname } = useLocation();
  return <Group<T> {...props} activeKey={pickActiveTab(props.items, pathname)} mode="link" />;
}

function Group<T extends string>({
  label,
  items,
  className,
  dataAttributes,
  activeKey,
  mode,
  onSelect,
}: Omit<ViewTabsProps<T>, 'value' | 'onValueChange'> & {
  activeKey: string | undefined;
  mode: 'link' | 'value';
  onSelect?: ((key: T) => void) | undefined;
}) {
  const { scrollerRef, contentRef, box, animated, fadeStyle, onKeyDown } = useTabStrip(activeKey);

  const strip = (
    <div
      ref={scrollerRef}
      role={mode === 'value' ? 'group' : undefined}
      aria-label={mode === 'value' ? label : undefined}
      className="tabs-scroll"
      style={fadeStyle}
      onKeyDown={onKeyDown}
    >
      <div ref={contentRef} className="relative inline-flex items-center gap-[2px]">
        <span
          aria-hidden="true"
          data-view-pill
          className={cn(
            'pointer-events-none absolute left-0 top-0 rounded-md bg-indigo-dim',
            animated &&
              'transition-[transform,width,height] duration-[280ms] ease-[cubic-bezier(0.32,0.72,0,1)]',
            box ? 'opacity-100' : 'opacity-0',
          )}
          style={
            box
              ? {
                  width: box.width,
                  height: box.height,
                  transform: `translate(${box.left}px, ${box.top}px)`,
                }
              : undefined
          }
        />
        {items.map((item) => (
          <View
            key={item.key}
            item={item}
            active={item.key === activeKey}
            mode={mode}
            onSelect={onSelect}
          />
        ))}
      </div>
    </div>
  );

  return (
    <div className={cn('min-w-0', className)} {...dataAttributes}>
      {mode === 'link' ? <nav aria-label={label}>{strip}</nav> : strip}
    </div>
  );
}

function View<T extends string>({
  item,
  active,
  mode,
  onSelect,
}: {
  item: ViewTabItem<T>;
  active: boolean;
  mode: 'link' | 'value';
  onSelect?: ((key: T) => void) | undefined;
}) {
  const className = cn(
    'relative z-10 inline-flex h-[28px] shrink-0 items-center gap-[6px] rounded-md px-[10px] font-sans text-xs outline-none transition-colors duration-fast [@media(pointer:coarse)]:h-[36px]',
    active
      ? 'text-indigo-soft'
      : 'text-tx-2 hover:bg-bg-2 hover:text-tx-0 active:bg-bg-3 focus-visible:bg-bg-3 focus-visible:text-tx-0',
  );
  const content = (
    <>
      {/* The bold label is laid out invisibly under the shown one, so the view
          keeps the width it has when selected and nothing shifts. */}
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
            'type-micro tabular-nums transition-colors duration-fast',
            active ? 'text-indigo-soft' : 'text-tx-3',
          )}
        >
          {item.count}
        </span>
      )}
    </>
  );

  if (mode === 'value') {
    return (
      <button
        type="button"
        data-tab
        data-tab-active={active}
        aria-pressed={active}
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
      aria-current={active ? 'page' : undefined}
      className={className}
    >
      {content}
    </Link>
  );
}
