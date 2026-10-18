import { useQuery } from '@tanstack/react-query';
import { ArrowUpRight, Bot } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { useAuthStore } from '@/stores/auth';

import type { StarterSelection } from './types';
import { greetingPeriodForHour } from '../../greeting';
import { discoverStarterService } from '../../starterService';

export interface QuickActionsProps {
  displayName: string;
  onPrime: (selection: StarterSelection) => void;
}

/** A way to start: a question put in the composer, or a page that does the job. */
interface Scene {
  key: string;
  title: string;
  hint: string;
  selection?: StarterSelection;
  to?: string;
}

/**
 * What a new chat offers to start with: the jobs of an SRE it is built for.
 * Investigate an incident, ask for data in plain words, cut alert noise, and
 * set up a scheduled inspection.
 */
export function QuickActions({ displayName, onPrime }: QuickActionsProps) {
  const { t } = useTranslation('agent');
  const navigate = useNavigate();
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const starterService = useQuery({
    queryKey: ['agent', 'starter-service', orgId],
    queryFn: () => discoverStarterService({ orgId }),
    enabled: Boolean(orgId),
    staleTime: 2 * 60_000,
    retry: false,
  });
  const targetService = starterService.data?.trim() || null;
  const queryPrompt = targetService
    ? t('quick.scenes.query.prompt_service', { service: targetService })
    : t('quick.scenes.query.prompt');
  const greetingPeriod = greetingPeriodForHour(new Date().getHours());
  const greetingKey = displayName
    ? `greeting.${greetingPeriod}_named`
    : `greeting.${greetingPeriod}`;

  const scenes: Scene[] = [
    {
      key: 'investigate-incident',
      title: t('quick.scenes.incident.title'),
      hint: t('quick.scenes.incident.prompt'),
      selection: { prompt: t('quick.scenes.incident.prompt'), mode: 'deep' },
    },
    {
      key: 'query-data',
      title: t('quick.scenes.query.title'),
      hint: queryPrompt,
      selection: {
        prompt: queryPrompt,
        ...(targetService ? { context: { service: targetService } } : {}),
        rangePreset: '1h',
      },
    },
    {
      key: 'alert-noise',
      title: t('quick.scenes.noise.title'),
      hint: t('quick.scenes.noise.hint'),
      selection: { prompt: '/alert-noise-reduction 7d' },
    },
    {
      key: 'scheduled-inspection',
      title: t('quick.scenes.inspection.title'),
      hint: t('quick.scenes.inspection.hint'),
      to: '/logs/inspections',
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
        <h3 className="type-micro font-strong tracking-normal text-tx-3">
          {t('quick_title')}
        </h3>
        <div
          data-testid="agent-quick-actions"
          className="mt-2 divide-y divide-bd-0 border-y border-bd-0"
        >
          {scenes.map((scene) => (
            <button
              key={scene.key}
              type="button"
              data-agent-scene={scene.key}
              onClick={() => {
                if (scene.selection) onPrime(scene.selection);
                else if (scene.to) navigate(scene.to);
              }}
              className="group flex min-h-12 w-full items-center gap-3 px-1 py-3 text-left text-base text-tx-1 transition-colors duration-fast hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0 sm:px-3 sm:text-sm"
            >
              <span className="min-w-0 flex-1">
                <span className="block font-strong text-tx-0">{scene.title}</span>
                <span className="mt-0.5 block truncate text-xs text-tx-3">{scene.hint}</span>
              </span>
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
