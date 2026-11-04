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
   * The half of the page this strip is in is the one worked in. On a split
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
  /**
   * Asked once, when the strip is put on the page: whether it takes the focus.
   * The page that comes forward lists the tabs in a strip of its own, and the
   * strip it took the place of may have had it.
   */
  claimFocus?: (() => boolean) | undefined;
  onSelect: (id: string) => void;
  onAdd: () => void;
  onClose: (id: string) => void;
  onCloseOthers: (id: string) => void;
  onDuplicate: (id: string) => void;
  onRename: (id: string, label: string) => void;
}

/**
 * The queries open in an explorer, as tabs in its query box, each a small
 * block of its own.
 *
 * Each tab is a query of its own: selecting one brings its page to the front
 * with its results as they were left. The one in front is tinted. A dot beside
 * a name means that tab's query is still running, wherever the tab is.
 *
 * A page split in two lists each half's tabs in that half's query box. The tab
 * each half shows keeps a fill of its own; the tint is for the half that is
 * worked in, the other is quieter.
 */
export function QueryTabStrip({
  tabs,
  activeId,
  focused = true,
  closable: closableProp,
  canAdd,
  limit,
  moveAcross,
  claimFocus,
  onSelect,
  onAdd,
  onClose,
  onCloseOthers,
  onDuplicate,
  onRename,
}: QueryTabStripProps) {
  const { t } = useTranslation('common');
  const { scrollerRef, contentRef, fadeStyle, onKeyDown } = useTabStrip(
    activeId,
    { activateOnArrow: true },
  );
  const [renaming, setRenaming] = React.useState<string | null>(null);
  const closable = closableProp ?? tabs.length > 1;

  React.useLayoutEffect(() => {
    if (!claimFocus?.()) return;
    scrollerRef.current?.querySelector<HTMLElement>('[data-tab-active="true"] [data-tab]')?.focus();
    // Only the strip just put on the page asks.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div
      data-query-tab-strip
      data-query-tab-strip-focused={focused}
      className="flex min-w-0 items-center gap-[4px]"
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
          {tabs.map((tab) => (
            <QueryTabPill
              key={tab.id}
              tab={tab}
              active={tab.id === activeId}
              focused={focused}
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
          className="ms-press grid h-[24px] w-[24px] shrink-0 place-items-center rounded-[5px] text-tx-2 transition-colors duration-fast enabled:hover:bg-bg-3 enabled:hover:text-tx-0 enabled:focus-visible:bg-bg-3 enabled:focus-visible:text-tx-0 disabled:cursor-not-allowed disabled:text-[var(--tx-disabled)]"
        >
          <Plus className="h-[14px] w-[14px]" aria-hidden="true" />
        </button>
      </DisabledControl>
    </div>
  );
}

/**
 * The fill of a tab. The one in front has the tint the bar marks a choice
 * with, where its half is worked in; every other is a gray block a step off
 * the field it stands on, in either theme.
 */
function blockClass(active: boolean, focused: boolean): string {
  if (!active) return 'bg-bg-3 text-tx-2 hover:bg-bg-4 hover:text-tx-0 focus-within:bg-bg-4 focus-within:text-tx-0';
  return focused ? 'bg-[var(--indigo-tint)] text-[var(--indigo-ink)]' : 'bg-bg-4 text-tx-0';
}

function QueryTabPill({
  tab,
  active,
  focused,
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
  focused: boolean;
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
          // The strip measures its tabs by this, and keeps the one in front in view.
          data-tab-active={active}
          className={cn(
            'group/qtab relative flex h-[24px] min-w-0 max-w-[160px] shrink-0 items-center rounded-[5px] transition-colors duration-fast',
            blockClass(active, focused),
          )}
          // Middle click closes, as on any tab.
          onAuxClick={(event) => {
            if (event.button !== 1 || !closable) return;
            event.preventDefault();
            onClose(tab.id);
          }}
        >
          {renaming ? (
            <RenameField name={tab.name ?? ''} placeholder={name} onDone={onRenameEnd} />
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
                'flex h-full min-w-0 items-center gap-[6px] rounded-[5px] pl-[8px] font-sans text-xs outline-none',
                closable ? 'pr-[2px]' : 'pr-[8px]',
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
                'mr-[3px] grid h-[18px] w-[18px] shrink-0 place-items-center rounded text-current outline-none transition-[opacity,color,background-color] duration-fast hover:bg-bg-4 hover:text-tx-0 focus-visible:bg-bg-4 focus-visible:text-tx-0',
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
      className="mx-[2px] h-[20px] w-[128px] min-w-0 rounded bg-[var(--control-surface)] px-[8px] font-sans text-xs text-tx-0 outline-none placeholder:text-tx-3"
    />
  );
}
