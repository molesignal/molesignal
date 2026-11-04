import { ChartNoAxesColumn } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { FunnelParams } from '@/api/rum';
import { widenTimeWindow } from '@/investigation/timeRangeRecovery';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';

import { FunnelBuilder } from './FunnelBuilder';
import { FunnelBars, FunnelGroups } from './FunnelResult';
import { isComplete, toStep } from './steps';
import { useFunnel } from './useAnalytics';
import { useFunnelAddress } from './useFunnelAddress';
import { isBrowser, scopeFilters } from '../overview/model';
import { CardState, OverviewCard } from '../overview/OverviewCard';
import type { ScopeContext } from '../ScopedFrame';

/**
 * How many sessions got through a series of steps, in order: the steps are chosen by what a page or
 * an action is called, the funnel says how many sessions did each after the one before and how long
 * they took, and the sessions can be counted by a field to see who gets through and who does not.
 * The steps, the window and the field are in the address, so a funnel is a link.
 */
export function FunnelView({ context }: { context: ScopeContext }) {
  const { t } = useTranslation('rum');
  const { application, scope, range, language, window, setWindow } = context;
  const browser = isBrowser(application.platform);
  const { steps, windowSecs, breakdown, setSteps, setWindow: setLimit, setBreakdown } = useFunnelAddress(browser);

  const ready = steps.every(isComplete);

  const request = React.useMemo<FunnelParams | null>(
    () =>
      ready
        ? {
            from: range.from,
            to: range.to,
            filters: scopeFilters(scope),
            steps: steps.map(toStep),
            ...(windowSecs && { windowSecs: windowSecs }),
          }
        : null,
    [ready, range, scope, steps, windowSecs],
  );

  const overall = useFunnel(request);
  const grouped = useFunnel(request && breakdown ? { ...request, breakdown } : null);

  const first = overall.data?.groups[0];
  const nobody = overall.isSuccess && !overall.data?.unavailable && (first?.steps[0]?.sessions ?? 0) === 0;

  return (
    <div data-rum-analytics="funnel" className="space-y-[12px]">
      <OverviewCard icon={ChartNoAxesColumn} title={t('analytics.funnel.title')} description={t('analytics.funnel.steps_description')} testId="funnel-steps">
        <FunnelBuilder
          steps={steps}
          onSteps={setSteps}
          windowSecs={windowSecs}
          onWindow={setLimit}
          breakdown={breakdown}
          onBreakdown={setBreakdown}
          browser={browser}
          scope={scope}
          range={range}
        />
      </OverviewCard>

      {!ready ? (
        <ProductState variant="empty" compact title={t('analytics.funnel.choose.title')} description={t('analytics.funnel.choose.description')} />
      ) : (
        <>
          <OverviewCard title={t('analytics.funnel.result')} description={t('analytics.funnel.result_description')} testId="funnel-result" tone="text-[var(--chart-2)]">
            <CardState query={overall} empty={nobody} emptyTitle={t('analytics.funnel.nobody')}>
              {first && <FunnelBars group={first} language={language} />}
            </CardState>
            {nobody && (
              <div className="flex justify-center pb-4">
                <ChromeButton onClick={() => setWindow(widenTimeWindow(window))}>{t('overview.empty.widen')}</ChromeButton>
              </div>
            )}
          </OverviewCard>
          {breakdown && !nobody && (
            <OverviewCard
              title={t('analytics.funnel.by_title', { field: t(`analytics.breakdowns.${breakdown.replaceAll('.', '_')}`) })}
              testId="funnel-groups"
              tone="text-[var(--chart-4)]"
            >
              <CardState query={grouped} empty={(grouped.data?.groups.length ?? 0) === 0} emptyTitle={t('analytics.funnel.nobody')}>
                {grouped.data && <FunnelGroups funnel={grouped.data} field={t(`analytics.breakdowns.${breakdown.replaceAll('.', '_')}`)} language={language} />}
              </CardState>
            </OverviewCard>
          )}
        </>
      )}
    </div>
  );
}
