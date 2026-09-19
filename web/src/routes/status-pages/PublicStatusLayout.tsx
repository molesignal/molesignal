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

import { statusPageLanguageLabel } from './model';
import { PublicStatusSubscribe } from './PublicStatusSubscribe';

export function PublicStatusLayout({
  page,
  language,
  languages,
  languageLabel,
  statusPageLabel,
  onLanguageChange,
  homeHref,
  poweredBy,
  footerNavigationLabel,
  footerLinks,
  children,
}: {
  page: PublicStatusPageInfo;
  language: StatusPageLanguage;
  languages: StatusPageLanguage[];
  languageLabel: string;
  statusPageLabel: string;
  onLanguageChange: (language: StatusPageLanguage) => void;
  homeHref: string;
  poweredBy: string;
  footerNavigationLabel: string;
  footerLinks: Array<{ href: string; label: string; current?: boolean }>;
  children: ReactNode;
}) {
  const style = { '--status-brand': page.brand_color } as CSSProperties;
  return (
    <main
      data-theme="light"
      lang={language}
      style={style}
      className="h-full overflow-y-auto overscroll-y-contain bg-bg-0 font-sans text-tx-1"
    >
      <div className="h-1 w-full bg-[var(--status-brand)]" />
      <header className="bg-white">
        <div className="mx-auto flex w-full max-w-[1100px] flex-col items-stretch gap-5 px-4 py-7 sm:flex-row sm:items-center sm:justify-between sm:px-6 sm:py-9 lg:px-8">
          <Link
            to={homeHref}
            className="flex min-w-0 items-center gap-3 rounded-md transition-colors focus-visible:bg-bg-2"
          >
            {page.logo_url ? (
              <img
                src={page.logo_url}
                alt=""
                className="h-12 w-12 shrink-0 rounded-lg border border-bd-0 bg-white object-contain p-1 sm:h-14 sm:w-14"
              />
            ) : (
              <span
                aria-hidden
                className="grid h-12 w-12 shrink-0 place-items-center rounded-lg text-lg font-bold text-white sm:h-14 sm:w-14"
                style={{ backgroundColor: page.brand_color }}
              >
                {page.name.slice(0, 1).toUpperCase()}
              </span>
            )}
            <div className="min-w-0">
              <h1 className="truncate text-2xl font-display-strong tracking-[-0.025em] text-tx-0 sm:text-3xl">
                {page.name}
              </h1>
              <p className="mt-1 text-sm text-tx-2">{statusPageLabel}</p>
            </div>
          </Link>

          <div
            data-testid="status-page-actions"
            className="flex w-full flex-row items-start justify-end gap-3 sm:w-auto sm:items-center"
          >
            <PublicStatusSubscribe
              slug={page.slug}
              language={language}
              privatePage={page.visibility === 'private'}
            />
            {languages.length > 1 && (
              <LanguageSwitcher
                languages={languages}
                value={language}
                label={languageLabel}
                onChange={onLanguageChange}
              />
            )}
          </div>
        </div>
      </header>

      <div className="mx-auto w-full max-w-[1100px] px-4 pb-16 pt-8 sm:px-6 sm:pb-20 sm:pt-12 lg:px-8">
        {children}
      </div>

      <footer className="border-t border-bd-0 bg-white">
        <div className="mx-auto flex w-full max-w-[1100px] flex-col gap-5 px-4 py-7 text-sm text-tx-2 sm:px-6 md:flex-row md:items-center md:justify-between lg:px-8">
          <nav
            aria-label={footerNavigationLabel}
            className="flex flex-wrap items-center gap-x-5 gap-y-2"
          >
            {footerLinks.map((link) => (
              <Link
                key={link.href}
                to={link.href}
                aria-current={link.current ? 'page' : undefined}
                className={cn(
                  'min-h-11 content-center rounded-md transition-colors hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0 md:min-h-0',
                  link.current && 'font-strong text-[var(--status-brand)]',
                )}
              >
                {link.label}
              </Link>
            ))}
          </nav>
          <span className="text-tx-3">{poweredBy}</span>
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
        className="h-11 w-auto min-w-40 bg-white text-base font-strong text-tx-1 sm:text-sm"
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
