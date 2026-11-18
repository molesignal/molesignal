import { ArrowUpRight } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link, useHref } from 'react-router-dom';

import type { ProductRouteMeta } from '@/product/ia';
import { cn } from '@/shell/lib/cn';

import { NavIcon } from './NavIcon';

/**
 * One area in the main menu: a real link, so a ⌘/Ctrl-click or a middle click opens it in a new
 * tab as anywhere on the web. Pointing at it shows a second way to the same: the arrow at its end
 * opens it in a new tab in one click. When the user has every entry open in a new tab, the link
 * itself does that and the arrow is not needed.
 */
export function MenuEntry({
  entry,
  current,
  newTab,
  onNavigate,
}: {
  entry: ProductRouteMeta;
  current: boolean;
  newTab: boolean;
  onNavigate: () => void;
}) {
  const { t } = useTranslation('nav');
  const label = t(entry.labelKey);
  const href = useHref(entry.path);

  return (
    <div className="group/entry relative flex min-w-0 items-center">
      <Link
        to={entry.path}
        target={newTab ? '_blank' : undefined}
        rel={newTab ? 'noopener noreferrer' : undefined}
        onClick={onNavigate}
        aria-current={current ? 'page' : undefined}
        data-menu-entry={entry.id}
        className={cn(
          'flex h-8 min-w-0 flex-1 items-center gap-2.5 rounded-md pl-2.5 font-sans text-sm transition-colors duration-fast [@media(pointer:coarse)]:h-11',
          newTab ? 'pr-2.5' : 'pr-9',
          current
            ? 'bg-[var(--nav-current-surface)] font-strong text-[var(--nav-current-ink)]'
            : 'text-[var(--nav-ink)] group-hover/entry:bg-[var(--nav-hover-surface)] group-hover/entry:text-[var(--nav-hover-ink)] focus-visible:bg-bg-3 focus-visible:text-tx-0',
        )}
      >
        <NavIcon icon={entry.icon} className={current ? 'text-[var(--nav-current-ink)]' : 'text-tx-2'} />
        <span className="min-w-0 truncate">{label}</span>
      </Link>
      {!newTab && (
        <a
          href={href}
          target="_blank"
          rel="noopener noreferrer"
          onClick={onNavigate}
          aria-label={t('menu.open_entry_in_new_tab', { name: label })}
          title={t('menu.open_in_new_tab')}
          data-menu-new-tab={entry.id}
          className="absolute right-1 grid h-6 w-6 place-items-center rounded text-tx-2 opacity-0 transition-opacity duration-fast hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 focus-visible:opacity-100 group-hover/entry:opacity-100 [@media(pointer:coarse)]:hidden"
        >
          <ArrowUpRight className="h-3.5 w-3.5" />
        </a>
      )}
    </div>
  );
}
