import { House } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link, useLocation } from 'react-router-dom';

import { chromeIconButtonSizeClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';

/**
 * The way back to the reader's home page, beside the menu button. It is a link, so a ⌘/Ctrl
 * click opens home in a new tab; on the home page itself it stands in the brand's tint, as the
 * menu's current entry does.
 */
export function HomeButton({ to }: { to: string }) {
  const { t } = useTranslation('nav');
  const { pathname } = useLocation();
  const current = pathname === to.split(/[?#]/)[0];
  const label = t('home');

  return (
    <Link
      to={to}
      data-testid="home-button"
      aria-label={label}
      aria-current={current ? 'page' : undefined}
      title={label}
      className={cn(
        'ms-press relative flex shrink-0 items-center justify-center rounded-md transition-colors duration-fast',
        chromeIconButtonSizeClass,
        current
          ? 'bg-[var(--nav-current-surface)] text-[var(--nav-current-ink)]'
          : 'text-tx-2 hover:bg-bg-3 hover:text-tx-0',
        'focus-visible:bg-indigo-dim focus-visible:text-indigo focus-visible:outline-none',
      )}
    >
      <House className="h-4 w-4" />
    </Link>
  );
}
