import { ArrowRight, Bot } from 'lucide-react';
import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { Skeleton } from '@/shell/ui/skeleton';

import {
  type ConditionTone,
  type FleetSummary,
  type HealthVerdictKind,
  VERDICT_TONE,
} from './streamCondition';

const TONE_COLOR: Record<ConditionTone, string> = {
  green: 'var(--green)',
  yellow: 'var(--yellow)',
  red: 'var(--red)',
  dim: 'var(--tx-3)',
};

const TONE_TEXT: Record<ConditionTone, string> = {
  green: 'text-green',
  yellow: 'text-yellow',
  red: 'text-red',
  dim: 'text-tx-0',
};

/** States worth a slow pulse on the dot: something is off, nothing is on fire. */
const PULSING_TONES: ReadonlySet<ConditionTone> = new Set(['red', 'yellow']);

type HeroAction = 'investigate' | 'viewStreams' | 'connect' | 'retry';

/** One primary next step per verdict: the page should never leave "so what now?" open. */
const VERDICT_ACTION: Record<HealthVerdictKind, HeroAction> = {
  no_streams: 'connect',
  no_data: 'connect',
  healthy: 'viewStreams',
  delayed: 'investigate',
  interrupted: 'investigate',
  stale: 'investigate',
  unknown: 'retry',
};

/** Verdicts whose copy has an "all N streams" variant. */
const HAS_ALL_VARIANT: ReadonlySet<HealthVerdictKind> = new Set([
  'healthy',
  'delayed',
  'interrupted',
  'stale',
]);

/** The agent offer only makes sense when something needs explaining. */
const OFFERS_AGENT: ReadonlySet<HealthVerdictKind> = new Set([
  'delayed',
  'interrupted',
  'stale',
  'unknown',
]);

export interface HealthHeroActions {
  investigate: () => void;
  viewStreams: () => void;
  connect: () => void;
  retry: () => void;
  /** Hands Mole Agent a question about what the verdict says; omit when the user has no agent access. */
  askAgent?: ((prompt: string) => void) | undefined;
}

/**
 * The Home verdict: one sentence about whether data is flowing, why that
 * matters, and the single thing to do next. It is deliberately a compact
 * strip, not a banner: the headline and its action share a row, the evidence
 * sits on the line below, and the signal line runs along the bottom edge.
 */
export function HealthHero({
  fleet,
  loading,
  lastReceived,
  actions,
  decoration,
  riseIndex = 0,
}: {
  fleet: FleetSummary | undefined;
  loading: boolean;
  /** Formatted last-receive time ("6天前"), or "—" when unknown. */
  lastReceived: string;
  actions: HealthHeroActions;
  /** Decorative graphic anchored to the card's lower edge (the signal line). */
  decoration?: React.ReactNode;
  riseIndex?: number | undefined;
}) {
  const { t } = useTranslation('onboarding');
  const kind: HealthVerdictKind = fleet?.verdict ?? 'unknown';
  const tone = VERDICT_TONE[kind];
  const copyKey = `home.hero.verdict.${kind}`;
  const detailKey =
    fleet?.uniform && HAS_ALL_VARIANT.has(kind) ? 'detail_all' : 'detail';
  const title = t(`${copyKey}.title`);
  const detail = t(`${copyKey}.${detailKey}`, {
    total: fleet?.total ?? 0,
    count: fleet?.affected ?? 0,
    receiving: fleet?.receiving ?? 0,
  });
  const primary = VERDICT_ACTION[kind];
  const showLastReceived =
    kind !== 'no_streams' && kind !== 'unknown' && lastReceived !== '—';

  return (
    <section
      aria-label={t('home.hero.eyebrow')}
      aria-busy={loading || undefined}
      data-home-surface="hero"
      data-verdict={loading ? 'loading' : kind}
      data-tone={tone}
      style={
        {
          '--tone': TONE_COLOR[tone],
          '--rise-index': riseIndex,
        } as React.CSSProperties
      }
      className="home-canvas-hero home-rise relative isolate flex min-w-0 flex-col justify-center overflow-hidden rounded-md bg-[var(--functional-surface)] px-[20px] pb-[26px] pt-[14px] [box-shadow:inset_3px_0_0_var(--tone),var(--shadow-functional-surface)]"
    >
      {decoration}
      <div className="relative z-10 flex flex-wrap items-center justify-between gap-x-4 gap-y-2.5">
        <div className="min-w-0 flex-1 basis-[240px]">
          <div className="flex items-center gap-2.5">
            <span
              aria-hidden="true"
              className={cn(
                'home-status-dot',
                PULSING_TONES.has(tone) && !loading && 'home-breathe',
              )}
            />
            {loading ? (
              <Skeleton className="h-6 w-44" />
            ) : (
              <h2
                className={cn(
                  'type-display min-w-0 text-balance font-sans font-display tracking-normal',
                  TONE_TEXT[tone],
                )}
              >
                {title}
              </h2>
            )}
          </div>
          {loading ? (
            <Skeleton className="mt-2 h-3.5 w-64 max-w-full" />
          ) : (
            <p className="type-data mt-1 text-pretty text-tx-1">
              {detail}
              {showLastReceived && (
                <span className="ml-1.5 text-tx-2">
                  {t('home.kpis.last_received', { when: lastReceived })}
                </span>
              )}
            </p>
          )}
        </div>

        {!loading && (
          <div className="flex shrink-0 flex-wrap items-center gap-2">
            <ChromeButton variant="primary" onClick={actions[primary]}>
              {t(`${copyKey}.action`)}
              <ArrowRight aria-hidden="true" className="h-3.5 w-3.5" />
            </ChromeButton>
            {actions.askAgent && OFFERS_AGENT.has(kind) && (
              <ChromeButton
                onClick={() =>
                  actions.askAgent?.(
                    // The detail is a sentence; inside the template it is a parenthetical.
                    t('home.hero.agent_prompt', {
                      title,
                      detail: detail.replace(/[。.]+$/u, ''),
                    }),
                  )
                }
              >
                <Bot aria-hidden="true" className="h-3.5 w-3.5 text-indigo-soft" />
                {t('home.hero.ask_agent')}
              </ChromeButton>
            )}
          </div>
        )}
      </div>
    </section>
  );
}
