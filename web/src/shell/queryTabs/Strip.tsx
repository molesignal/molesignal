import { Plus, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DisabledControl } from '@/shell/DisabledControl';
import { cn } from '@/shell/lib/cn';
import { useTabStrip } from '@/shell/tabs/useTabStrip';
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '@/shell/ui/context-menu';

import '@/shell/tabs/tabs.css';

export interface QueryTabStripItem {
  id: string;
  /** The name to show; a tab without one is numbered. */
  name: string | null;
  ordinal: number;
  running: boolean;
  tabId: string;
  panelId: string;
}

interface QueryTabStripProps {
  tabs: readonly QueryTabStripItem[];
  activeId: string;
  /**
   * The half of the page under this strip is the one worked in. On a split
   * page each half has its strip, and only one of them is.
   */
  focused?: boolean | undefined;
  /** A tab can be closed. Without it, a strip of one tab keeps that tab. */
  closable?: boolean | undefined;
  /** Fewer tabs than the limit: one more can be opened. */
  canAdd: boolean;
  limit: number;
  /** Moves a tab to the other half of a split page; named by where that is. */
  moveAcross?: { label: string; onMove: (id: string) => void } | undefined;
  /** What stands at the far end of the strip. */
  end?: React.ReactNode;
  onSelect: (id: string) => void;
  onAdd: () => void;
  onClose: (id: string) => void;
  onCloseOthers: (id: string) => void;
  onDuplicate: (id: string) => void;
  onRename: (id: string, label: string) => void;
}

/**
 * The queries open in an explorer, as tabs above its workbench.
 *
 * Each tab is a query of its own: selecting one brings its page to the front
 * with its results as they were left. The surface under the selected tab is the
 * same one the workbench below is drawn on, and it glides to the tab that is
 * picked. A dot beside a name means that tab's query is still running, wherever
 * the tab is.
 *
 * A page split in two has a strip over each half, listing that half's tabs.
 * The tab each half shows keeps a surface; the surface of the half that is
 * worked in is the one the workbench is drawn on, the other is quieter.
 */
export function QueryTabStrip({
  tabs,
  activeId,
  focused = true,
  closable: closableProp,
  canAdd,
  limit,
  moveAcross,
  end,
  onSelect,
  onAdd,
  onClose,
  onCloseOthers,
  onDuplicate,
  onRename,
}: QueryTabStripProps) {
  const { t } = useTranslation('common');
  const { scrollerRef, contentRef, box, animated, fadeStyle, onKeyDown } = useTabStrip(
    activeId,
    { activateOnArrow: true },
  );
  const [renaming, setRenaming] = React.useState<string | null>(null);
  const closable = closableProp ?? tabs.length > 1;

  return (
    <div
      data-query-tab-strip
      // Close under the top bar and close over the page: the strip names the page under it.
      data-query-tab-strip-focused={focused}
      className="flex shrink-0 items-center gap-[4px] pl-[var(--pane-gutter-start,20px)] pr-[var(--pane-gutter-end,20px)] pt-[4px]"
    >
      <div
        ref={scrollerRef}
        role="tablist"
        aria-label={t('query_tabs.list')}
        className="tabs-scroll min-w-0"
        style={fadeStyle}
        onKeyDown={renaming ? undefined : onKeyDown}
      >
        <div ref={contentRef} className="relative flex items-center gap-[4px]">
          <span
            aria-hidden="true"
            data-query-tab-surface
            className={cn(
              'pointer-events-none absolute left-0 top-0 rounded-md',
              // The half that is worked in has the workbench's own surface under its tab.
              focused
                ? 'bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]'
                : 'bg-bg-3',
              animated &&
                'transition-[transform,width] duration-[280ms] ease-[cubic-bezier(0.32,0.72,0,1)]',
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
          {tabs.map((tab) => (
            <QueryTabPill
              key={tab.id}
              tab={tab}
              active={tab.id === activeId}
              moveAcross={moveAcross}
              closable={closable}
              canDuplicate={canAdd}
              renaming={renaming === tab.id}
              onSelect={onSelect}
              onClose={onClose}
              onCloseOthers={onCloseOthers}
              onDuplicate={onDuplicate}
              onRenameStart={setRenaming}
              onRenameEnd={(label) => {
                setRenaming(null);
                if (label !== null) onRename(tab.id, label);
              }}
            />
          ))}
        </div>
      </div>
      <DisabledControl
        disabled={!canAdd}
        reason={t('query_tabs.limit', { count: limit })}
      >
        <button
          type="button"
          data-testid="query-tab-add"
          aria-label={t('query_tabs.new')}
          title={canAdd ? t('query_tabs.new') : undefined}
          disabled={!canAdd}
          onClick={onAdd}
          className="ms-press grid h-[28px] w-[28px] shrink-0 place-items-center rounded-md text-tx-2 transition-colors duration-fast enabled:hover:bg-bg-2 enabled:hover:text-tx-0 enabled:focus-visible:bg-bg-3 enabled:focus-visible:text-tx-0 disabled:cursor-not-allowed disabled:text-tx-3"
        >
          <Plus className="h-[14px] w-[14px]" aria-hidden="true" />
        </button>
      </DisabledControl>
      {end ? <div className="ml-auto flex shrink-0 items-center">{end}</div> : null}
    </div>
  );
}

function QueryTabPill({
  tab,
  active,
  moveAcross,
  closable,
  canDuplicate,
  renaming,
  onSelect,
  onClose,
  onCloseOthers,
  onDuplicate,
  onRenameStart,
  onRenameEnd,
}: {
  tab: QueryTabStripItem;
  active: boolean;
  moveAcross?: { label: string; onMove: (id: string) => void } | undefined;
  closable: boolean;
  canDuplicate: boolean;
  renaming: boolean;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onCloseOthers: (id: string) => void;
  onDuplicate: (id: string) => void;
  onRenameStart: (id: string) => void;
  /** `null` leaves the name as it was. */
  onRenameEnd: (label: string | null) => void;
}) {
  const { t } = useTranslation('common');
  const name = tab.name ?? t('query_tabs.untitled', { number: tab.ordinal });

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <div
          data-query-tab={tab.id}
          // The strip measures this element to place the surface under it.
          data-tab-active={active}
          className={cn(
            'group/qtab relative z-10 flex h-[28px] min-w-0 max-w-[220px] shrink-0 items-center rounded-md transition-colors duration-fast',
            active
              ? 'text-tx-0'
              : 'text-tx-2 hover:bg-bg-2 hover:text-tx-0 focus-within:bg-bg-2 focus-within:text-tx-0',
          )}
          // Middle click closes, as on any tab.
          onAuxClick={(event) => {
            if (event.button !== 1 || !closable) return;
            event.preventDefault();
            onClose(tab.id);
          }}
        >
          {renaming ? (
            <RenameField
              name={tab.name ?? ''}
              placeholder={name}
              onDone={onRenameEnd}
            />
          ) : (
            <button
              type="button"
              role="tab"
              id={tab.tabId}
              data-tab
              aria-selected={active}
              aria-controls={tab.panelId}
              tabIndex={active ? 0 : -1}
              title={name}
              onClick={() => onSelect(tab.id)}
              onDoubleClick={() => onRenameStart(tab.id)}
              onKeyDown={(event) => {
                if (event.key === 'F2') {
                  event.preventDefault();
                  onRenameStart(tab.id);
                }
              }}
              className={cn(
                'flex h-full min-w-0 items-center gap-[6px] rounded-md pl-[10px] font-sans text-xs outline-none',
                closable ? 'pr-[2px]' : 'pr-[10px]',
              )}
            >
              {tab.running && (
                <span
                  data-query-tab-running
                  role="img"
                  aria-label={t('query_tabs.running')}
                  className="ms-live h-[6px] w-[6px] shrink-0 rounded-full bg-indigo"
                />
              )}
              {/* The bold name is laid out invisibly under the shown one, so a
                  tab keeps its width when it is selected. */}
              <span className="inline-grid min-w-0">
                <span
                  className={cn(
                    'col-start-1 row-start-1 truncate',
                    active ? 'font-strong' : 'font-body',
                  )}
                >
                  {name}
                </span>
                <span
                  aria-hidden="true"
                  className="invisible col-start-1 row-start-1 truncate font-strong"
                >
                  {name}
                </span>
              </span>
            </button>
          )}
          {closable && !renaming && (
            <button
              type="button"
              aria-label={t('query_tabs.close', { name })}
              tabIndex={active ? 0 : -1}
              onClick={() => onClose(tab.id)}
              className={cn(
                'mr-[4px] grid h-[20px] w-[20px] shrink-0 place-items-center rounded text-tx-3 outline-none transition-[opacity,color,background-color] duration-fast hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0',
                // Always there for the selected tab; the others show it when approached.
                active
                  ? 'opacity-100'
                  : 'opacity-0 focus-visible:opacity-100 group-hover/qtab:opacity-100',
              )}
            >
              <X className="h-[12px] w-[12px]" aria-hidden="true" />
            </button>
          )}
        </div>
      </ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem onSelect={() => onRenameStart(tab.id)}>
          {t('query_tabs.rename')}
        </ContextMenuItem>
        <ContextMenuItem disabled={!canDuplicate} onSelect={() => onDuplicate(tab.id)}>
          {t('query_tabs.duplicate')}
        </ContextMenuItem>
        {moveAcross ? (
          <ContextMenuItem onSelect={() => moveAcross.onMove(tab.id)}>{moveAcross.label}</ContextMenuItem>
        ) : null}
        <ContextMenuSeparator />
        <ContextMenuItem disabled={!closable} onSelect={() => onClose(tab.id)}>
          {t('query_tabs.close_tab')}
        </ContextMenuItem>
        <ContextMenuItem disabled={!closable} onSelect={() => onCloseOthers(tab.id)}>
          {t('query_tabs.close_others')}
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

function RenameField({
  name,
  placeholder,
  onDone,
}: {
  name: string;
  placeholder: string;
  onDone: (label: string | null) => void;
}) {
  const { t } = useTranslation('common');
  const [draft, setDraft] = React.useState(name);
  const inputRef = React.useRef<HTMLInputElement>(null);
  // Enter and Escape both blur the field; only the first of the three counts.
  const settled = React.useRef(false);
  const finish = (label: string | null) => {
    if (settled.current) return;
    settled.current = true;
    onDone(label);
  };

  React.useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  return (
    <input
      ref={inputRef}
      value={draft}
      placeholder={placeholder}
      aria-label={t('query_tabs.name')}
      maxLength={60}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={() => finish(draft)}
      onKeyDown={(event) => {
        // The strip's arrow keys move between tabs; here they move the caret.
        event.stopPropagation();
        if (event.key === 'Enter') finish(draft);
        if (event.key === 'Escape') finish(null);
      }}
      className="mx-[3px] h-[22px] w-[160px] min-w-0 rounded bg-[var(--control-surface)] px-[8px] font-sans text-xs text-tx-0 outline-none placeholder:text-tx-3"
    />
  );
}
