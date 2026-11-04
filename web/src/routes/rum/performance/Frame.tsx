import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ProductState } from '@/product/states';

import { readView } from './address';
import { PerformanceViews } from './Views';
import { RumInvestigateButton } from '../agent/InvestigateButton';
import { isBrowser } from '../overview/model';
import { type ScopeContext, ScopedFrame } from '../ScopedFrame';

/** What a view of the performance tab is of, and what it needs to ask. */
export type PerformanceContext = ScopeContext;

/**
 * The frame every view of the performance tab stands in: the tabs, the views under them, the
 * application and environment and the range of time they are of. The vitals, the loading of
 * resources and the long tasks are measured by the browser SDK: an application of another kind has
 * none to show. The head offers Mole Agent on the tab: on the vital in front where the tab is of the
 * vitals, on the performance of the pages where it is not.
 */
export function PerformanceFrame({
  vitals = false,
  children,
}: {
  /** Whether the tab is the Web Vitals, which have one in front. */
  vitals?: boolean;
  children: (context: PerformanceContext) => React.ReactNode;
}) {
  const { t } = useTranslation('rum');

  return (
    <ScopedFrame
      name="performance"
      toolbar={(context, params) => (
        <RumInvestigateButton
          subject={{
            application: { id: context.application.id, name: context.application.name },
            environment: context.scope.environment,
            range: context.range,
            focus: vitals ? { kind: 'vital', vital: readView(params).metric } : { kind: 'performance' },
          }}
        />
      )}
      views={(params, application) => (isBrowser(application.platform) ? <PerformanceViews params={params} /> : undefined)}
      unsupported={(application) =>
        isBrowser(application.platform) ? null : (
          <ProductState
            variant="empty"
            title={t('performance.not_browser.title')}
            description={t('performance.not_browser.description')}
          />
        )
      }
    >
      {children}
    </ScopedFrame>
  );
}
