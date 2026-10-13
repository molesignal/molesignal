import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Outlet, useLocation, useSearchParams } from 'react-router-dom';

import * as meApi from '@/api/me';
import { setActiveTimePreference } from '@/lib/time';
import {
  canAccessProductPath,
  isKnownProductPath,
  useProductAccess,
} from '@/product/access';
import { USER_PREFERENCES_QUERY_KEY } from '@/shell/PreferenceRuntime';
import { useTheme } from '@/shell/ThemeBootstrap';
import { hydrateFromSearchParams } from '@/shell/UrlHydration';
import { useAuthStore } from '@/stores/auth';
import { useThemeStore } from '@/stores/useThemeStore';

import { ReportUnavailable } from './Document';
import { reportTimezone } from './mode';

const THEME_ATTRIBUTE = 'data-theme';
const DOCUMENT_ATTRIBUTE = 'data-report-document';

/**
 * What a signed-in route is rendered in when it is being printed for a report,
 * in place of the application shell.
 *
 * The shell is for working in the product: navigation, the top bar, the
 * command palette, a page that scrolls inside the window. A report is the
 * route's content and nothing else, on a document as long as that content.
 * What the route needs from the shell is kept: the time range the URL names,
 * the reader's language and date format, and the access check.
 */
export function ReportRenderFrame() {
  const { t } = useTranslation(['common', 'iam']);
  const [search] = useSearchParams();
  const location = useLocation();
  const token = useAuthStore((state) => state.token);
  const access = useProductAccess();
  const setLanguage = useThemeStore((state) => state.setLanguage);
  const timezone = reportTimezone(search);
  const [personalized, setPersonalized] = React.useState(false);
  const preferencesQuery = useQuery({
    queryKey: USER_PREFERENCES_QUERY_KEY,
    queryFn: () => meApi.preferences(),
    enabled: Boolean(token),
    staleTime: 5 * 60_000,
  });
  const preferences = preferencesQuery.data;
  const preferencesSettled = preferencesQuery.isFetched;

  useReportDocument();

  React.useLayoutEffect(() => {
    hydrateFromSearchParams(search);
  }, [location.pathname, search]);

  // The content is held back until these are in: it is printed once, so it has to be right the
  // first time. Theme and density are left alone, since a report looks the same for everyone.
  React.useLayoutEffect(() => {
    if (!preferencesSettled) return;

    if (preferences) setLanguage(preferences.language);
    const formats = preferences ?? meApi.DEFAULT_USER_PREFERENCES;
    setActiveTimePreference(
      timezone ?? formats.timezone,
      formats.time_format,
      formats.date_format,
    );
    setPersonalized(true);
  }, [preferences, preferencesSettled, setLanguage, timezone]);

  if (!personalized || !access || access.status === 'loading') return null;

  if (access.status === 'error') {
    return <ReportUnavailable reason={t('iam:capabilities.load_error')} />;
  }

  if (
    isKnownProductPath(location.pathname, access) &&
    !canAccessProductPath(location.pathname, access)
  ) {
    return <ReportUnavailable reason={t('common:access.page_unavailable')} />;
  }

  return <Outlet />;
}

/**
 * Makes the document paper while a report is on it: as long as its content,
 * where the application keeps it to the window, and light whatever the reader's
 * theme is.
 */
function useReportDocument(): void {
  const { theme } = useTheme();
  const appTheme = React.useRef(theme);

  React.useEffect(() => {
    appTheme.current = theme;
  }, [theme]);

  React.useLayoutEffect(() => {
    const root = document.documentElement;
    const themed = [root, document.body];
    const lighten = () => {
      for (const element of themed) {
        if (element.getAttribute(THEME_ATTRIBUTE) !== 'light') {
          element.setAttribute(THEME_ATTRIBUTE, 'light');
        }
      }
    };

    root.setAttribute(DOCUMENT_ATTRIBUTE, '');
    lighten();
    // The application writes the theme again when the preference or the system changes.
    const observer = new MutationObserver(lighten);
    for (const element of themed) {
      observer.observe(element, { attributes: true, attributeFilter: [THEME_ATTRIBUTE] });
    }

    return () => {
      observer.disconnect();
      root.removeAttribute(DOCUMENT_ATTRIBUTE);
      for (const element of themed) element.setAttribute(THEME_ATTRIBUTE, appTheme.current);
    };
  }, []);
}
