import { Menu, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { chromeIconButtonSizeClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { IconSwap } from '@/shell/polish/IconSwap';

/** The id the menu's panel carries, which the button names as what it opens. */
export const MAIN_MENU_ID = 'main-menu';

/**
 * The button the whole navigation hangs from, beside the organization in the top bar. While the
 * menu is open it stands in the brand's tint and shows the cross that closes it.
 */
export function MenuButton({ open, onToggle }: { open: boolean; onToggle: () => void }) {
  const { t } = useTranslation('nav');
  const label = t(open ? 'menu.close' : 'menu.open');

  return (
    <button
      type="button"
      data-testid="main-menu-trigger"
      aria-expanded={open}
      aria-controls={MAIN_MENU_ID}
      aria-label={label}
      title={label}
      onClick={onToggle}
      className={cn(
        'ms-press relative flex shrink-0 items-center justify-center rounded-md transition-colors duration-fast',
        chromeIconButtonSizeClass,
        open
          ? 'bg-[var(--nav-current-surface)] text-[var(--nav-current-ink)]'
          : 'text-tx-2 hover:bg-bg-3 hover:text-tx-0',
        'focus-visible:bg-indigo-dim focus-visible:text-indigo focus-visible:outline-none',
      )}
    >
      <IconSwap active={open} inactiveIcon={<Menu className="h-4 w-4" />} activeIcon={<X className="h-4 w-4" />} />
    </button>
  );
}
