import type { TFunction } from 'i18next';
import { Globe2 } from 'lucide-react';
import type { CSSProperties, ReactNode } from 'react';
import { Link } from 'react-router-dom';

import type {
  PublicStatusPage as PublicStatusPageInfo,
  StatusPageLanguage,
} from '@/api/statusPages';
import { cn } from '@/shell/lib/cn';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';

import { statusPageLanguageLabel } from '../model';
import { PUBLIC_CONTROL_HEIGHT } from './control';
import { PublicStatusCredit } from './Credit';
import type { PublicStatusPaths, PublicStatusView } from './routing';
import { PublicStatusSubscribe } from './Subscribe';

/**
 * The frame every view of a public status page stands in: whose page it is,
 * the three places it has (what is going on now, what happened, how it has
 * held up), a way to be told of changes, and what serves it.
 *
 * The page is the publisher's. Their mark and colour lead; the navigation is
 * in the header of every view, so none of them is a dead end.
 */
export function PublicStatusLayout({
  page,
  view,
  paths,
  language,
  languages,
  copy,
  onLanguageChange,
  children,
}: {
  page: PublicStatusPageInfo;
  /** The view being shown; an incident belongs to none of the three places. */
  view: PublicStatusView;
  paths: PublicStatusPaths;
  language: StatusPageLanguage;
  languages: StatusPageLanguage[];
  /** The page's copy, in the language it is being read in. */
  copy: TFunction;
  onLanguageChange: (language: StatusPageLanguage) => void;
  children: ReactNode;
}) {
  const style = { '--status-brand': page.brand_color } as CSSProperties;
  const places = [
    { view: 'status', href: paths.current, label: copy('public.archive.current_status') },
    { view: 'history', href: paths.history, label: copy('public.archive.incident_history') },
    { view: 'uptime', href: paths.uptime, label: copy('public.archive.uptime') },
  ] as const;

  return (
    <main
      data-theme="light"
      lang={language}
      style={style}
      // A column as tall as the window at least: on a page with little to say, the footer
      // stands at the foot of the window and not halfway down it.
      className="flex h-full flex-col overflow-y-auto overscroll-y-contain bg-bg-0 font-sans text-tx-1"
    >
      <div className="h-1 w-full shrink-0 bg-[var(--status-brand)]" />
      {/* The header's lower edge is drawn inside it, so the line under the place one is in can
          lie on that edge without a link having to hang out of the navigation's box. */}
      <header className="shrink-0 bg-white shadow-[inset_0_-1px_0_var(--bd-0)]">
        <div className="mx-auto flex w-full max-w-[1100px] flex-col items-stretch gap-5 px-4 pb-5 pt-6 sm:flex-row sm:items-center sm:justify-between sm:px-6 sm:pt-8 lg:px-8">
          <Link
            to={paths.current}
            className="flex min-w-0 items-center gap-3 rounded-md transition-colors focus-visible:bg-bg-2"
          >
            {page.logo_url ? (
              <img
                src={page.logo_url}
                alt=""
                className="h-11 w-11 shrink-0 rounded-lg border border-bd-0 bg-white object-contain p-1 sm:h-12 sm:w-12"
              />
            ) : (
              <span
                aria-hidden
                className="grid h-11 w-11 shrink-0 place-items-center rounded-lg text-lg font-bold text-white sm:h-12 sm:w-12"
                style={{ backgroundColor: page.brand_color }}
              >
                {page.name.slice(0, 1).toUpperCase()}
              </span>
            )}
            <div className="min-w-0">
              <h1 className="truncate text-xl font-display-strong tracking-[-0.02em] text-tx-0 sm:text-2xl">
                {page.name}
              </h1>
              <p className="mt-0.5 text-sm text-tx-2">{copy('public.status_page_label')}</p>
            </div>
          </Link>

          <div
            data-testid="status-page-actions"
            className="flex w-full flex-row items-start justify-start gap-3 sm:w-auto sm:items-center sm:justify-end"
          >
            <PublicStatusSubscribe
              slug={page.slug}
              language={language}
              brandColor={page.brand_color}
              privatePage={page.visibility === 'private'}
            />
            {languages.length > 1 && (
              <LanguageSwitcher
                languages={languages}
                value={language}
                label={copy('public.language_switcher')}
                onChange={onLanguageChange}
              />
            )}
          </div>
        </div>

        <nav
          aria-label={copy('public.archive.navigation_label')}
          // Scrolls sideways on the narrowest screens, and shows no bar of its own in either direction.
          className="mx-auto flex w-full max-w-[1100px] gap-1 overflow-x-auto overflow-y-hidden px-2 [scrollbar-width:none] sm:px-4 lg:px-6 [&::-webkit-scrollbar]:hidden"
        >
          {places.map((place) => (
            <Link
              key={place.view}
              to={place.href}
              aria-current={view === place.view ? 'page' : undefined}
              className={cn(
                'min-h-[44px] shrink-0 content-center whitespace-nowrap border-b-2 border-transparent px-2 text-sm font-strong text-tx-2 transition-colors hover:text-tx-0 focus-visible:bg-bg-1 focus-visible:text-tx-0 sm:px-2.5',
                view === place.view && 'border-[var(--status-brand)] text-tx-0',
              )}
            >
              {place.label}
            </Link>
          ))}
        </nav>
      </header>

      <div className="mx-auto w-full max-w-[1100px] flex-1 px-4 pb-16 pt-8 sm:px-6 sm:pb-20 sm:pt-10 lg:px-8">
        {children}
      </div>

      <footer className="shrink-0 border-t border-bd-0 bg-white">
        <div className="mx-auto flex w-full max-w-[1100px] justify-center px-4 py-5 sm:justify-end sm:px-6 lg:px-8">
          <PublicStatusCredit label={copy('public.powered_by')} />
        </div>
      </footer>
    </main>
  );
}

function LanguageSwitcher({
  languages,
  value,
  label,
  onChange,
}: {
  languages: StatusPageLanguage[];
  value: StatusPageLanguage;
  label: string;
  onChange: (language: StatusPageLanguage) => void;
}) {
  return (
    <Select
      value={value}
      onValueChange={(language) => onChange(language as StatusPageLanguage)}
    >
      <SelectTrigger
        aria-label={label}
        // As wide as what it says: the mark, the language and the arrow stand together.
        className={cn(PUBLIC_CONTROL_HEIGHT, 'w-auto shrink-0 gap-2 text-base font-strong text-tx-1 sm:text-sm')}
      >
        <span className="flex min-w-0 items-center gap-2">
          <Globe2 aria-hidden className="h-4 w-4 shrink-0 text-tx-3" />
          <SelectValue />
        </span>
      </SelectTrigger>
      <SelectContent align="end" data-theme="light">
        {languages.map((item) => (
          <SelectItem key={item} value={item} className="min-h-10">
            {statusPageLanguageLabel(item)}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
