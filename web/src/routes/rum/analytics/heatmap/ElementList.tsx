import { useTranslation } from 'react-i18next';

import type { ClickedElement } from '@/api/rum';

import { formatCount } from '../../overview/format';

/**
 * The elements people clicked, the most clicked first: how often, in how many sessions, and how many of
 * those clicks came with a sign of frustration. An element the recording behind the map does not have
 * says so: its clicks are placed by where on the page they landed.
 */
export function ElementList({
  elements,
  off,
  language,
}: {
  elements: readonly ClickedElement[];
  off: ReadonlySet<string>;
  language: string;
}) {
  const { t } = useTranslation('rum');

  return (
    <ol aria-label={t('analytics.heatmap.elements')} className="m-0 max-h-[calc(100vh-260px)] list-none overflow-auto p-0">
      {elements.map((element) => {
        const name = element.name || element.selector || element.element;

        return (
          <li key={element.element} data-heat-element={element.element} className="border-b border-[var(--row-rule)] px-3 py-2 font-sans text-xs">
            <div className="flex items-baseline justify-between gap-3">
              <span className="min-w-0 truncate font-strong text-tx-0" title={name}>
                {name}
              </span>
              <span data-heat-clicks="" className="shrink-0 tabular-nums text-tx-0">
                {formatCount(element.clicks, language)}
              </span>
            </div>
            <div className="mt-0.5 flex flex-wrap items-baseline gap-x-3 text-tx-3">
              <span>{t('analytics.heatmap.sessions', { count: element.sessions })}</span>
              {element.frustrated > 0 && (
                <span data-heat-frustrated="" className="text-[var(--orange-ink)]">
                  {t('analytics.heatmap.frustrated', { count: element.frustrated })}
                </span>
              )}
              {off.has(element.element) && <span data-heat-off="">{t('analytics.heatmap.off_snapshot')}</span>}
            </div>
          </li>
        );
      })}
    </ol>
  );
}
