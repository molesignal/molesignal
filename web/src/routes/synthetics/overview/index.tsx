import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

import { SyntheticsActivation } from '../Activation';
import { useSyntheticsWorkspace } from '../data';
import {
  CreateCheckAction,
  SyntheticsPage,
  SyntheticsSection,
  useLoadState,
} from '../Frame';
import { WorldAvailabilityMap } from '../map/WorldAvailabilityMap';
import { checksNeedingAttention } from '../summary';
import { ChecksNeedingAttention } from './Attention';
import { ChecksStanding } from './Standing';

/**
 * What the checks say now.
 *
 * The tab opens with one statement, then what that statement rests on: the
 * checks that are not passing, by name, and how the latest runs went from each
 * place they ran. Every check and every run has a tab of its own, so nothing
 * here is a second copy of those lists. A part with nothing in it is not drawn:
 * the statement has already said there is nothing.
 */
export function SyntheticsOverview() {
  const { t } = useTranslation('synthetics');
  const workspace = useSyntheticsWorkspace({ resultLimit: 30 });
  const state = useLoadState({
    pending: workspace.pending,
    error: workspace.error,
    onRetry: () => void workspace.refetch(),
  });
  // Until there is a check there is nothing to state: the tab says how to begin.
  const firstRun = !state && workspace.rows.length === 0;
  const attention = checksNeedingAttention(workspace.rows);

  return (
    <SyntheticsPage
      // The first-run page carries the way to a first check itself.
      action={firstRun ? undefined : <CreateCheckAction />}
      lead={
        !state && !firstRun ? (
          <ChecksStanding
            rows={workspace.rows}
            results={workspace.operationalResults}
            locations={workspace.locations}
          />
        ) : undefined
      }
      state={state}
      contentSurface={false}
    >
      {firstRun ? (
        <SyntheticsActivation />
      ) : (
        <div
          className={cn(
            'grid grid-cols-1 items-start gap-[12px]',
            attention.length > 0 && 'xl:grid-cols-[minmax(0,1fr)_minmax(0,1.25fr)]',
          )}
        >
          {attention.length > 0 && (
            <ChecksNeedingAttention
              items={attention}
              locations={workspace.locations}
              nowMicros={Date.now() * 1000}
            />
          )}
          <SyntheticsSection
            title={t('by_location.title')}
            description={t('by_location.description')}
          >
            <WorldAvailabilityMap
              locations={workspace.locations}
              results={workspace.operationalResults}
            />
          </SyntheticsSection>
        </div>
      )}
    </SyntheticsPage>
  );
}
