import * as React from 'react';

import { cn } from '@/shell/lib/cn';

type QueryToolbarTone = 'indigo' | 'orange';

const activeToneClass: Record<QueryToolbarTone, string> = {
  indigo: 'bg-indigo text-white shadow-sm',
  orange: 'bg-orange text-[var(--orange-fg)] shadow-sm',
};

interface QueryWorkbenchProps {
  /** The page's name, when the workbench bar carries it instead of a title row. */
  title?: React.ReactNode;
  toolbar: React.ReactNode;
  /** Page-level actions pinned to the end of the toolbar row: time range, run, refresh. */
  actions?: React.ReactNode;
  children: React.ReactNode;
  /**
   * The body is a one-line summary right now (a collapsed editor). When the
   * toolbar row has room for it, the summary sits in that row, between the
   * controls and the actions, and the card is a single row instead of two.
   */
  inlineBody?: boolean;
  className?: string;
  bodyClassName?: string;
  footer?: React.ReactNode;
  appearance?: 'default' | 'surface';
}

/** The narrowest a one-line query summary can be and still show its text. */
const INLINE_BODY_MIN_WIDTH = 320;

/** Width a wrapping row of controls needs to sit on one line. */
function singleLineWidth(element: HTMLElement): number {
  const items = Array.from(element.children) as HTMLElement[];
  const gap = Number.parseFloat(getComputedStyle(element).columnGap) || 0;
  return (
    items.reduce((width, item) => width + item.offsetWidth, 0) +
    gap * Math.max(0, items.length - 1)
  );
}

/**
 * Whether a one-line body fits in the toolbar row beside the controls and the
 * actions. It is measured, not guessed from the viewport: the room depends on
 * the sidebar, the locale and how many controls the page puts in the row.
 */
function useInlineBodyFit(
  enabled: boolean,
  rowRef: React.RefObject<HTMLDivElement>,
  startRef: React.RefObject<HTMLDivElement>,
  endRef: React.RefObject<HTMLDivElement>,
): boolean {
  const [fits, setFits] = React.useState(false);

  React.useLayoutEffect(() => {
    const row = rowRef.current;
    const start = startRef.current;
    if (!enabled || !row || !start) {
      setFits(false);
      return;
    }

    const measure = () => {
      const end = endRef.current;
      const gap = Number.parseFloat(getComputedStyle(row).columnGap) || 0;
      const needed =
        singleLineWidth(start) +
        gap +
        INLINE_BODY_MIN_WIDTH +
        (end ? gap + singleLineWidth(end) : 0);
      setFits(row.clientWidth >= needed);
    };

    measure();
    if (typeof ResizeObserver === 'undefined') return;
    // The row resizes with the window and the sidebar; the controls resize when
    // a label changes ("Run" to "Running…", a tab count, the locale).
    const observer = new ResizeObserver(measure);
    observer.observe(row);
    observer.observe(start);
    if (endRef.current) observer.observe(endRef.current);
    return () => observer.disconnect();
  }, [enabled, rowRef, startRef, endRef]);

  return enabled && fits;
}

/**
 * The pointer and keyboard fill of the collapsed editor on the surface card.
 * The pages draw it there as a field, on the control surface, so it takes the
 * step past that fill as an input does. The frame's own hover and the global
 * focus fallback are both `--bg-2`: the dark theme's control surface itself.
 * It is set on the card because the editor is in the toolbar row only while it
 * fits there, and under the row otherwise.
 */
const COLLAPSED_EDITOR_HOVER_FOCUS_CLASS =
  '[&_[data-query-editor-state=collapsed]>button:hover]:bg-bg-3 [&_[data-query-editor-state=collapsed]>button:focus-visible]:bg-bg-3';

export function QueryWorkbench({
  title,
  toolbar,
  actions,
  children,
  inlineBody = false,
  className,
  bodyClassName,
  footer,
  appearance = 'default',
}: QueryWorkbenchProps) {
  const surface = appearance === 'surface';
  const rowRef = React.useRef<HTMLDivElement>(null);
  const startRef = React.useRef<HTMLDivElement>(null);
  const endRef = React.useRef<HTMLDivElement>(null);
  const inline = useInlineBodyFit(inlineBody, rowRef, startRef, endRef);

  return (
    <section
      data-query-workbench-appearance={appearance}
      data-query-body={inline ? 'inline' : 'stacked'}
      className={cn(
        surface
          // The page's gutter on either side: 20px, less towards the other half of a split page.
          ? 'ml-[var(--pane-gutter-start,20px)] mr-[var(--pane-gutter-end,20px)] mb-[12px] rounded-md bg-[var(--functional-surface)] px-[12px] py-[8px] [box-shadow:var(--shadow-functional-surface)]'
          : 'border-b border-bd-0 bg-bg-1 p-3',
        surface && COLLAPSED_EDITOR_HOVER_FOCUS_CLASS,
        className,
      )}
    >
      <div
        ref={rowRef}
        data-query-toolbar
        className={cn(
          'flex w-full flex-wrap items-center',
          surface
            ? 'min-h-10 gap-[8px] rounded-md bg-transparent'
            : 'min-h-11 gap-2 rounded-lg border border-bd-1 bg-bg-2/80 px-2 py-1.5 shadow-sm',
        )}
      >
        <div
          ref={startRef}
          className={cn('flex min-w-0 flex-wrap items-center', surface ? 'gap-[8px]' : 'gap-2')}
        >
          {title}
          {toolbar}
        </div>
        {inline && (
          // The collapsed editor as a field in the row: the same height and
          // corner as the controls beside it.
          <div
            data-query-inline-body
            className="min-w-0 flex-1 [&_[data-query-editor-state=collapsed]>button]:h-9 [&_[data-query-editor-state=collapsed]]:rounded-md"
          >
            {children}
          </div>
        )}
        {actions && (
          <div
            ref={endRef}
            className="ml-auto flex shrink-0 flex-wrap items-center justify-end gap-1.5"
          >
            {actions}
          </div>
        )}
      </div>
      {!inline && (
        <div className={cn(surface ? 'mt-[8px]' : 'mt-3', bodyClassName)}>{children}</div>
      )}
      {footer ? (
        <div
          className={cn(
            'mt-3 rounded-lg px-3 py-2',
            surface ? 'bg-[var(--control-surface)]' : 'border border-bd-1 bg-bg-1',
          )}
        >
          {footer}
        </div>
      ) : null}
    </section>
  );
}

interface QueryToolbarGroupProps extends React.HTMLAttributes<HTMLDivElement> {
  children: React.ReactNode;
}

export function QueryToolbarGroup({ children, className, ...props }: QueryToolbarGroupProps) {
  return (
    <div
      {...props}
      className={cn('flex h-9 shrink-0 items-center gap-0.5 rounded-md border border-bd-0 bg-bg-1 p-0.5', className)}
    >
      {children}
    </div>
  );
}

interface QueryToolbarButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  active?: boolean;
  tone?: QueryToolbarTone;
  flat?: boolean;
}

export const QueryToolbarButton = React.forwardRef<
  HTMLButtonElement,
  QueryToolbarButtonProps
>(function QueryToolbarButton({
  active = false,
  tone = 'indigo',
  flat = false,
  className,
  children,
  type = 'button',
  ...props
}, ref) {
  return (
    <button
      {...props}
      ref={ref}
      type={type}
      className={cn(
        'inline-flex h-8 shrink-0 items-center justify-center gap-1.5 whitespace-nowrap rounded px-3 font-sans text-xs font-strong text-tx-2 transition-colors disabled:cursor-not-allowed disabled:opacity-50',
        // The chosen one keeps its fill under the pointer: right after the click that chose it
        // the pointer is still there, and a grey hover would read as nothing having happened.
        active ? activeToneClass[tone] : 'hover:bg-bg-3 hover:text-tx-0',
        flat && 'shadow-none',
        className,
      )}
    >
      {children}
    </button>
  );
});

export interface QueryToolbarTab<T extends string> {
  id: T;
  label: React.ReactNode;
  count?: number | string | undefined;
}

interface QueryToolbarTabsProps<T extends string> {
  tabs: Array<QueryToolbarTab<T>>;
  activeId: T;
  onChange: (id: T) => void;
  tone?: QueryToolbarTone;
  flat?: boolean;
  selectionStyle?: 'fill' | 'underline';
}

export function QueryToolbarTabs<T extends string>({
  tabs,
  activeId,
  onChange,
  tone = 'indigo',
  flat = false,
  selectionStyle = 'fill',
}: QueryToolbarTabsProps<T>) {
  const underline = selectionStyle === 'underline';

  return (
    <QueryToolbarGroup
      role="tablist"
      data-query-tab-selection={selectionStyle}
      className={cn(
        'shrink-0',
        underline
          ? 'h-10 items-stretch gap-0 rounded-none border-0 bg-transparent p-0'
          : flat && 'border-0 bg-[var(--control-surface)]',
      )}
    >
      {tabs.map((tab) => {
        const active = tab.id === activeId;
        return (
          <QueryToolbarButton
            key={tab.id}
            role="tab"
            aria-selected={active}
            active={!underline && active}
            tone={tone}
            flat={flat || underline}
            onClick={() => onChange(tab.id)}
            className={cn(
              'min-w-[72px]',
              underline &&
                'relative h-10 rounded-none bg-transparent px-3 shadow-none after:absolute after:inset-x-3 after:bottom-0 after:h-[3px] after:rounded-t-md after:bg-transparent hover:bg-transparent hover:text-tx-0',
              underline && active && 'text-indigo-soft after:bg-indigo',
            )}
          >
            <span className="truncate">{tab.label}</span>
            {tab.count !== undefined ? (
              <span className="grid min-w-5 place-items-center rounded-full bg-bg-3 px-1.5 text-xs text-tx-2">
                {tab.count}
              </span>
            ) : null}
          </QueryToolbarButton>
        );
      })}
    </QueryToolbarGroup>
  );
}
