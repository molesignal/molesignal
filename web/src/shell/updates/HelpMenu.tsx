import { ArrowUpCircle, BookOpen, HelpCircle, Info, Keyboard } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { chromeIconButtonSizeClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/shell/ui/dropdown-menu';
import { Kbd } from '@/shell/ui/kbd';

import { useReleaseUpdates } from './useReleaseUpdates';

export function HelpMenu({
  title,
  docsLabel,
  keyboardLabel,
  aboutLabel,
  onDocs,
  onKeyboard,
  onAbout,
}: {
  title: string;
  docsLabel: string;
  keyboardLabel: string;
  aboutLabel: string;
  onDocs: () => void;
  onKeyboard: () => void;
  onAbout: () => void;
}) {
  const { t } = useTranslation('shell');
  const updates = useReleaseUpdates();
  const available = updates.data?.updateAvailable;

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          title={title}
          aria-label={title}
          className={cn(
            'ms-press relative hidden items-center justify-center rounded-md text-tx-2 md:flex',
            chromeIconButtonSizeClass,
            'hover:bg-bg-3 hover:text-tx-0',
            'focus-visible:bg-indigo-dim focus-visible:text-indigo focus-visible:outline-none',
          )}
        >
          <HelpCircle className="h-4 w-4" />
          {available && <span data-testid="release-update-dot" aria-hidden="true" className="absolute right-1 top-1 h-1.5 w-1.5 rounded-full bg-red" />}
          {available && <span className="sr-only">{t('about.updates.menu')}</span>}
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className={available ? 'w-72' : 'w-56'}>
        <DropdownMenuItem onSelect={onDocs}>
          <BookOpen className="h-4 w-4" />
          <span>{docsLabel}</span>
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={onAbout}>
          <Info className="h-4 w-4" />
          <span>{aboutLabel}</span>
        </DropdownMenuItem>
        {available && (
          <DropdownMenuItem onSelect={onAbout}>
            <ArrowUpCircle className="h-4 w-4 text-indigo" />
            <span>{t('about.updates.menu')}</span>
            <span className="ml-auto text-xs text-tx-3">v{updates.data?.latestVersion}</span>
          </DropdownMenuItem>
        )}
        <DropdownMenuItem onSelect={onKeyboard}>
          <Keyboard className="h-4 w-4" />
          <span>{keyboardLabel}</span>
          <Kbd variant="plain" className="ml-auto text-xs">?</Kbd>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

