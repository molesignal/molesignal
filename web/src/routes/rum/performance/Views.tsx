import { useTranslation } from 'react-i18next';

import { ViewTabs } from '@/shell/tabs/ViewTabs';

/** The views of the performance tab, in the order the tab tells them: the vitals, what is loaded, what blocks. */
export const PERFORMANCE_VIEWS = [
  { key: 'vitals', path: '/rum/performance' },
  { key: 'resources', path: '/rum/performance/resources' },
  { key: 'long_tasks', path: '/rum/performance/long-tasks' },
] as const;

/**
 * What the views carry from one to the other: the application and the environment. The range of time
 * is the reader's, kept apart from the address by the page; what a view asks of its own (the level in
 * front, a grouping, a value open) is of that view only.
 */
export function viewSearch(params: URLSearchParams): string {
  const kept = new URLSearchParams();

  for (const name of ['app', 'env']) {
    const value = params.get(name);

    if (value) kept.set(name, value);
  }

  const search = kept.toString();

  return search ? `?${search}` : '';
}

/** Which view of the performance tab is open, as a level of the navigation under the tabs. */
export function PerformanceViews({ params }: { params: URLSearchParams }) {
  const { t } = useTranslation('rum');
  const search = viewSearch(params);

  return (
    <div data-rum-subnavigation="performance" className="flex h-[40px] items-center px-[10px] [@media(pointer:coarse)]:h-[44px]">
      <ViewTabs
        label={t('performance.views.aria')}
        items={PERFORMANCE_VIEWS.map((view) => ({
          key: view.key,
          to: { pathname: view.path, search },
          label: t(`performance.views.${view.key}`),
        }))}
      />
    </div>
  );
}
