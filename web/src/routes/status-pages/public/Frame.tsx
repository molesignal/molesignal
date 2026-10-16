import { useQuery } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { useParams, useSearchParams } from 'react-router-dom';

import type {
  PublicStatusPage as PublicStatusPageInfo,
  PublicStatusPageSnapshot,
  StatusPageLanguage,
} from '@/api/statusPages';
import { toApiError } from '@/lib/http';

import { resolveStatusPageLanguage, statusPageLanguages } from '../model';
import { PublicStatusAccess } from './Access';
import { PublicStatusLayout } from './Layout';
import {
  type PublicStatusPaths,
  type PublicStatusSource,
  type PublicStatusView,
  publicStatusPageQuery,
  publicStatusPaths,
} from './routing';
import { PublicStatusState } from './State';

/** What a view of a public status page is given to show. */
export interface PublicStatusContext {
  snapshot: PublicStatusPageSnapshot;
  page: PublicStatusPageInfo;
  source: PublicStatusSource;
  slug: string;
  language: StatusPageLanguage;
  /** The page's copy in that language, whatever language the app itself is in. */
  copy: TFunction;
  paths: PublicStatusPaths;
  /** The `?lang=` every link inside the page carries along. */
  languageQuery: string;
}

/**
 * Loads a public status page and frames one of its views.
 *
 * Every view reads the same snapshot and has the same ways to fail: it is
 * still loading, the host is not a status page's, the page is private and
 * asks who is there, or there is no such page. Those are decided once here,
 * so the views only say what they are of.
 */
export function PublicStatusFrame({
  source,
  view,
  unmatchedDomain,
  children,
}: {
  source: PublicStatusSource;
  view: PublicStatusView;
  /** What to show on a host that turns out not to be a status page's. */
  unmatchedDomain?: ReactNode;
  children: (context: PublicStatusContext) => ReactNode;
}) {
  const { slug = '' } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const { t, i18n } = useTranslation('status-pages');
  const query = useQuery(publicStatusPageQuery(source, slug));

  if (query.isLoading) {
    return <PublicStatusState credit={t('public.powered_by')}>{t('states.loading')}</PublicStatusState>;
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
      <PublicStatusState title={t('public.not_found_title')} credit={t('public.powered_by')}>
        {error.status === 404 ? t('public.not_found_description') : t('public.load_error')}
      </PublicStatusState>
    );
  }

  const snapshot = query.data;
  const { page } = snapshot;
  const language = resolveStatusPageLanguage(page.language, page.languages, searchParams.get('lang'));
  const copy = i18n.getFixedT(language, 'status-pages');
  const paths = publicStatusPaths(source, slug, language);

  return (
    <PublicStatusLayout
      page={page}
      components={snapshot.components}
      view={view}
      paths={paths}
      language={language}
      languages={statusPageLanguages(page.language, page.languages)}
      copy={copy}
      onLanguageChange={(nextLanguage) => {
        const next = new URLSearchParams(searchParams);
        next.set('lang', nextLanguage);
        setSearchParams(next, { replace: true });
      }}
    >
      {children({
        snapshot,
        page,
        source,
        slug,
        language,
        copy,
        paths,
        languageQuery: `?lang=${encodeURIComponent(language)}`,
      })}
    </PublicStatusLayout>
  );
}
