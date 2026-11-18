import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'react-router-dom';

import { isProductRouteActive, type ProductRouteMeta } from '@/product/ia';
import { cn } from '@/shell/lib/cn';
import { Kbd } from '@/shell/ui/kbd';
import { Switch } from '@/shell/ui/switch';

import { useMenuEntries } from './entries';
import { MAIN_MENU_ID } from './MenuButton';
import { MenuEntry } from './MenuEntry';
import { useMenuPreference } from './preference';

/** How many of the latest areas the menu offers, besides the one open now. */
const RECENT_SHOWN = 5;

const EYEBROW = 'type-eyebrow px-2.5 pb-1.5 font-sans text-tx-2';

/**
 * The whole navigation, dropped from under the top bar by the menu button: Home and the areas
 * opened lately at its left, every other area in its group's column, and at its foot the way to
 * search and to open entries in new tabs. The page under it is dimmed; a click there, Escape, a
 * Tab out of it or opening an entry closes it, and Escape gives the focus back to the button.
 */
export function MainMenu({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = useTranslation('nav');
  const { pathname } = useLocation();
  const { home, columns, byId } = useMenuEntries();
  const { recent, newTab, setNewTab } = useMenuPreference();
  const panel = React.useRef<HTMLDivElement>(null);
  const switchId = React.useId();

  React.useEffect(() => {
    if (!open) return;

    const node = panel.current;
    const target = node?.querySelector<HTMLElement>('a[aria-current="page"]') ?? node?.querySelector<HTMLElement>('a[data-menu-entry]');
    target?.focus({ preventScroll: true });
    const trigger = () => document.querySelector<HTMLElement>('[data-testid="main-menu-trigger"]');
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;

      event.preventDefault();
      onClose();
      trigger()?.focus();
    };
    // A press anywhere else, the top bar included, closes the menu; the button toggles it itself.
    const onPointer = (event: PointerEvent) => {
      const at = event.target as Node;
      if (!node?.contains(at) && !trigger()?.contains(at)) onClose();
    };
    document.addEventListener('keydown', onKey);
    document.addEventListener('pointerdown', onPointer);

    return () => {
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('pointerdown', onPointer);
    };
  }, [open, onClose]);

  if (!open) return null;

  const isCurrent = (entry: ProductRouteMeta) => isProductRouteActive(entry, pathname);
  const latest = recent
    .map((id) => byId.get(id))
    .filter((entry): entry is ProductRouteMeta => !!entry && !isCurrent(entry) && entry.group !== 'home')
    .slice(0, RECENT_SHOWN);
  const entry = (item: ProductRouteMeta) => (
    <MenuEntry key={item.id} entry={item} current={isCurrent(item)} newTab={newTab} onNavigate={onClose} />
  );
  const aside = home.length > 0 || latest.length > 0;

  return (
    <>
      <div
        aria-hidden
        onClick={onClose}
        className="fixed inset-x-0 bottom-0 top-topbar z-40 bg-[color-mix(in_srgb,var(--overlay)_40%,transparent)] duration-150 animate-in fade-in-0 motion-reduce:animate-none"
      />
      <div
        id={MAIN_MENU_ID}
        ref={panel}
        data-testid="main-menu"
        onBlur={(event) => {
          const next = event.relatedTarget as Node | null;
          const trigger = document.querySelector('[data-testid="main-menu-trigger"]');
          if (next && !event.currentTarget.contains(next) && !trigger?.contains(next)) onClose();
        }}
        className="fixed inset-x-0 top-topbar z-40 flex max-h-[calc(100dvh-var(--topbar-h))] flex-col overflow-hidden rounded-b-lg bg-[var(--functional-surface)] [box-shadow:var(--shadow-popup)] duration-200 ease-out animate-in fade-in-0 slide-in-from-top-2 motion-reduce:animate-none"
      >
        <nav
          aria-label={t('primary_navigation')}
          className={cn(
            'grid min-h-0 gap-6 overflow-y-auto px-[20px] pb-4 pt-4',
            aside && 'md:grid-cols-[228px_minmax(0,1fr)] md:gap-7',
          )}
        >
          {aside && (
            <div className="min-w-0 md:border-r md:border-bd-0 md:pr-5">
              {home.length > 0 && <div className="grid gap-0.5">{home.map(entry)}</div>}
              {latest.length > 0 && (
                <section aria-label={t('recent')} className={cn(home.length > 0 && 'mt-[18px]')}>
                  <h2 className={cn(EYEBROW, 'm-0')}>{t('recent')}</h2>
                  <div className="grid gap-0.5">{latest.map(entry)}</div>
                </section>
              )}
            </div>
          )}
          <div
            style={{ '--menu-columns': columns.length } as React.CSSProperties}
            className="grid gap-x-8 gap-y-6 sm:grid-cols-2 lg:[grid-template-columns:repeat(var(--menu-columns),minmax(0,216px))]"
          >
            {columns.map((column) => (
              <div key={column.map((group) => group.group).join('+')} className="grid min-w-0 content-start gap-[18px]">
                {column.map((group) => (
                  <section key={group.group} aria-label={t(group.labelKey)}>
                    <h2 className={cn(EYEBROW, 'm-0')}>{t(group.labelKey)}</h2>
                    <div className="grid gap-0.5">{group.entries.map(entry)}</div>
                  </section>
                ))}
              </div>
            ))}
          </div>
        </nav>
        <div className="flex shrink-0 flex-wrap items-center gap-x-5 gap-y-2 border-t border-bd-0 px-[20px] py-2.5 font-sans text-xs text-tx-3">
          <span className="inline-flex items-center gap-1.5">
            <Kbd variant="plain" className="font-strong text-tx-2">⌘ K</Kbd>
            {t('menu.search_hint')}
          </span>
          {!newTab && <span className="hidden md:inline">{t('menu.new_tab_hint')}</span>}
          <span className="ml-auto inline-flex items-center gap-2 text-tx-2">
            <label htmlFor={switchId} className="cursor-pointer">{t('menu.always_new_tab')}</label>
            <Switch id={switchId} data-testid="main-menu-new-tab" checked={newTab} onCheckedChange={setNewTab} />
          </span>
        </div>
      </div>
    </>
  );
}
