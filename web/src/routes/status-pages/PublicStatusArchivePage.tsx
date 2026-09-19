import { useQuery } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { TriangleAlert } from 'lucide-react';
import { type ReactNode, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useParams, useSearchParams } from 'react-router-dom';

import { toApiError } from '@/lib/http';
import { cn } from '@/shell/lib/cn';

import { ArchiveMonthNavigator } from './ArchiveMonthNavigator';
import {
  resolveStatusPageLanguage,
  statusPageHistoryDays,
  statusPageLanguages,
} from './model';
import { PublicIncidentHistory } from './PublicIncidentHistory';
import { PublicStatusAccess } from './PublicStatusAccess';
import { archiveMonthWindow } from './publicStatusHistory';
import { PublicStatusLayout } from './PublicStatusLayout';
import {
  type PublicStatusSource,
  publicStatusPageQuery,
  publicStatusPaths,
} from './publicStatusRouting';
import { PublicUptimeHistory } from './PublicUptimeHistory';

type ArchiveView = 'history' | 'uptime';

export function PublicStatusHistoryPage({
  source = 'slug',
  unmatchedDomain,
}: {
  source?: PublicStatusSource;
  unmatchedDomain?: ReactNode;
}) {
  return (
    <PublicStatusArchivePage
      view="history"
      source={source}
      unmatchedDomain={unmatchedDomain}
    />
  );
}

export function PublicStatusUptimePage({
  source = 'slug',
  unmatchedDomain,
}: {
  source?: PublicStatusSource;
  unmatchedDomain?: ReactNode;
}) {
  return (
    <PublicStatusArchivePage
      view="uptime"
      source={source}
      unmatchedDomain={unmatchedDomain}
    />
  );
}

function PublicStatusArchivePage({
  view,
  source,
  unmatchedDomain,
}: {
  view: ArchiveView;
  source: PublicStatusSource;
  unmatchedDomain?: ReactNode;
}) {
  const { slug = '' } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const [archiveEndMonth, setArchiveEndMonth] = useState<number | null>(null);
  const { t, i18n } = useTranslation('status-pages');
  const query = useQuery(publicStatusPageQuery(source, slug));

  if (query.isLoading) {
    return <ArchiveLoadState>{t('states.loading')}</ArchiveLoadState>;
  }
  if (source === 'domain' && query.data === null) {
    return <>{unmatchedDomain}</>;
  }
  if (!query.data) {
    const error = toApiError(query.error);
    if (error.status === 401) {
      return <PublicStatusAccess source={source} slug={slug} />;
    }
    return (
      <ArchiveLoadState title={t('public.not_found_title')}>
        {error.status === 404
          ? t('public.not_found_description')
          : t('public.load_error')}
      </ArchiveLoadState>
    );
  }

  const snapshot = query.data;
  const { page } = snapshot;
  const languages = statusPageLanguages(page.language, page.languages);
  const language = resolveStatusPageLanguage(
    page.language,
    page.languages,
    searchParams.get('lang'),
  );
  const copy = i18n.getFixedT(language, 'status-pages');
  const paths = publicStatusPaths(source, slug, language);
  const languageQuery = `?lang=${encodeURIComponent(language)}`;
  const monthWindow = archiveMonthWindow(
    snapshot.generated_at,
    statusPageHistoryDays(page),
    page.timezone,
    archiveEndMonth,
  );

  return (
    <PublicStatusLayout
      page={page}
      language={language}
      languages={languages}
      languageLabel={copy('public.language_switcher')}
      statusPageLabel={copy('public.status_page_label')}
      onLanguageChange={(nextLanguage) => {
        const next = new URLSearchParams(searchParams);
        next.set('lang', nextLanguage);
        setSearchParams(next, { replace: true });
      }}
      homeHref={paths.current}
      poweredBy={copy('public.powered_by')}
      footerNavigationLabel={copy('public.archive.navigation_label')}
      footerLinks={[
        { href: paths.current, label: copy('public.archive.current_status') },
        {
          href: paths.history,
          label: copy('public.archive.incident_history'),
          current: view === 'history',
        },
        {
          href: paths.uptime,
          label: copy('public.archive.uptime'),
          current: view === 'uptime',
        },
      ]}
    >
      <ArchiveTabs view={view} paths={paths} copy={copy} />
      <ArchiveMonthNavigator
        window={monthWindow}
        language={language}
        copy={copy}
        onPrevious={() => setArchiveEndMonth(monthWindow.endIndex - 1)}
        onNext={() => setArchiveEndMonth(monthWindow.endIndex + 1)}
      />
      {view === 'history' ? (
        <PublicIncidentHistory
          snapshot={snapshot}
          source={source}
          slug={slug}
          timezone={page.timezone}
          language={language}
          languageQuery={languageQuery}
          monthKeys={monthWindow.keys}
          copy={copy}
        />
      ) : (
        <PublicUptimeHistory
          snapshot={snapshot}
          timezone={page.timezone}
          language={language}
          monthKeys={monthWindow.keys}
          copy={copy}
        />
      )}
    </PublicStatusLayout>
  );
}

function ArchiveTabs({
  view,
  paths,
  copy,
}: {
  view: ArchiveView;
  paths: { history: string; uptime: string };
  copy: TFunction;
}) {
  return (
    <nav
      aria-label={copy('public.archive.view_navigation_label')}
      className="flex border-b border-bd-0"
    >
      {(
        [
          ['history', paths.history, copy('public.archive.incidents')],
          ['uptime', paths.uptime, copy('public.archive.uptime')],
        ] as const
      ).map(([value, href, label]) => (
        <Link
          key={value}
          to={href}
          aria-current={view === value ? 'page' : undefined}
          className={cn(
            'min-h-11 border-b-2 border-transparent px-4 py-3 text-base font-strong text-tx-3 transition-colors hover:text-tx-0 focus-visible:bg-bg-1',
            view === value && 'border-[var(--status-brand)] text-tx-0',
          )}
        >
          {label}
        </Link>
      ))}
    </nav>
  );
}

function ArchiveLoadState({
  title,
  children,
}: {
  title?: string;
  children: ReactNode;
}) {
  return (
    <main
      data-theme="light"
      className="grid h-full min-h-screen place-items-center overflow-y-auto bg-bg-0 px-6 text-center"
    >
      <div className="max-w-md">
        {title && (
          <>
            <TriangleAlert
              aria-hidden
              className="mx-auto h-9 w-9 text-tx-3"
              strokeWidth={1.5}
            />
            <h1 className="mt-5 text-2xl font-display-strong text-tx-0">{title}</h1>
          </>
        )}
        <p className={cn('text-sm leading-relaxed text-tx-2', title && 'mt-2')}>
          {children}
        </p>
      </div>
    </main>
  );
}
