import { useQuery } from '@tanstack/react-query';
import { ArrowUpRight, Bot } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { useAuthStore } from '@/stores/auth';

import type { StarterSelection } from './types';
import { dashboardStarterSelection } from '../../dashboard-authoring/starter';
import { greetingPeriodForHour } from '../../greeting';
import { discoverStarterService } from '../../starterService';

export interface QuickActionsProps {
  displayName: string;
  onPrime: (selection: StarterSelection) => void;
}

export function QuickActions({ displayName, onPrime }: QuickActionsProps) {
  const { t } = useTranslation('agent');
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const starterService = useQuery({
    queryKey: ['agent', 'starter-service', orgId],
    queryFn: () => discoverStarterService({ orgId }),
    enabled: Boolean(orgId),
    staleTime: 2 * 60_000,
    retry: false,
  });
  const targetService = starterService.data?.trim() || null;
  const serviceErrorPrompt = targetService
    ? t('quick.service_errors', { service: targetService })
    : t('quick.service_errors_fallback');
  const greetingPeriod = greetingPeriodForHour(new Date().getHours());
  const greetingKey = displayName
    ? `greeting.${greetingPeriod}_named`
    : `greeting.${greetingPeriod}`;

  const suggestions: Array<{
    key: string;
    label: string;
    selection: StarterSelection;
  }> = [
    {
      key: 'service-errors',
      label: serviceErrorPrompt,
      selection: {
        prompt: serviceErrorPrompt,
        ...(targetService
          ? { context: { service: targetService } }
          : {}),
        rangePreset: '1h',
        mode: 'deep',
      },
    },
    {
      key: 'recent-anomalies',
      label: t('quick.recent_anomalies'),
      selection: {
        prompt: t('quick.recent_anomalies'),
        rangePreset: '30m',
        mode: 'deep',
      },
    },
    {
      key: 'active-alerts',
      label: t('quick.unacknowledged_alerts'),
      selection: {
        prompt: t('quick.unacknowledged_alerts'),
        mode: 'quick',
      },
    },
    {
      key: 'build-dashboard',
      label: t('quick.build_dashboard'),
      selection: dashboardStarterSelection(t('quick.build_dashboard')),
    },
  ];

  return (
    <section
      aria-labelledby="agent-workspace-title"
      className="mx-auto flex min-h-full w-full max-w-[720px] flex-col justify-center px-1 py-8 sm:px-4"
    >
      <div className="flex items-center gap-2 type-caption font-strong text-indigo">
        <span className="grid h-8 w-8 place-items-center rounded-md bg-indigo/10">
          <Bot aria-hidden="true" className="h-4 w-4" strokeWidth={1.8} />
        </span>
        {t(greetingKey, { name: displayName })}
      </div>

      <h2
        id="agent-workspace-title"
        className="mt-5 type-page-title font-display-strong tracking-[-0.025em] text-tx-0"
      >
        {t('workspace.start_title')}
      </h2>
      <p className="mt-2 max-w-xl text-base leading-7 text-tx-2 sm:text-sm sm:leading-6">
        {t('workspace.start_description')}
      </p>

      <div className="mt-7">
        <h3 className="type-micro font-strong uppercase tracking-[0.08em] text-tx-3">
          {t('quick_title')}
        </h3>
        <div
          data-testid="agent-quick-actions"
          className="mt-2 divide-y divide-bd-0 border-y border-bd-0"
        >
          {suggestions.map((suggestion) => (
            <button
              key={suggestion.key}
              type="button"
              onClick={() => onPrime(suggestion.selection)}
              className="group flex min-h-12 w-full items-center gap-3 px-1 py-3 text-left text-base text-tx-1 transition-colors duration-fast hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0 sm:px-3 sm:text-sm"
            >
              <span className="min-w-0 flex-1">{suggestion.label}</span>
              <ArrowUpRight
                aria-hidden="true"
                className="h-4 w-4 shrink-0 text-tx-4 transition-colors duration-fast group-hover:text-indigo group-focus-visible:text-indigo"
              />
            </button>
          ))}
        </div>
      </div>
    </section>
  );
}
