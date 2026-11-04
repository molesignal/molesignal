import { useTranslation } from 'react-i18next';

import { ViewTabs } from '@/shell/tabs/ViewTabs';

import { SEGMENT_PARAM } from './segment';
import { viewSearch } from '../performance/Views';

/** The views of the analytics tab, in the order a question grows: who gets through, where they go, who comes back, and what they do on a page. */
export const ANALYTICS_VIEWS = [
  { key: 'funnel', path: '/rum/analytics' },
  { key: 'paths', path: '/rum/analytics/paths' },
  { key: 'retention', path: '/rum/analytics/retention' },
  { key: 'heatmap', path: '/rum/analytics/heatmaps' },
] as const;

/** What a view of the tab is opened with: the application, the environment, and the segment. */
function viewsSearch(params: URLSearchParams): string {
  const kept = new URLSearchParams(viewSearch(params));
  const segment = params.get(SEGMENT_PARAM);

  if (segment) kept.set(SEGMENT_PARAM, segment);

  const search = kept.toString();

  return search ? `?${search}` : '';
}

/** Which view of the analytics tab is open, as a level of the navigation under the tabs. */
export function AnalyticsViews({ params }: { params: URLSearchParams }) {
  const { t } = useTranslation('rum');
  const search = viewsSearch(params);

  return (
    <div data-rum-subnavigation="analytics" className="flex h-[40px] items-center px-[10px] [@media(pointer:coarse)]:h-[44px]">
      <ViewTabs
        label={t('analytics.views.aria')}
        items={ANALYTICS_VIEWS.map((view) => ({
          key: view.key,
          to: { pathname: view.path, search },
          label: t(`analytics.views.${view.key}`),
        }))}
      />
    </div>
  );
}
