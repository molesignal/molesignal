import { ArrowUpRight } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import type * as homeApi from '@/api/home';
import * as rumIntakeApi from '@/api/rumIntake';
import { toApiError } from '@/lib/http';
import { Pill, type PillTone } from '@/shell/chrome';

import { intakePathForSignal } from '../../streams/datasourceLink';
import { sourceIcon } from '../catalogue/sourceIcon';
import { summarizeSourceSignals } from '../datasourceModel';
import { isValidRumApplicationId, substitute, useIntakeContext } from '../intakeContext';
import { ApplicationField } from '../mobileRum/ApplicationField';
import type { Signal, Source } from '../sources';
import { useSourceText } from '../sourceText';
import { ConnectionDetails, type GuideEndpoint } from './ConnectionDetails';
import { probeHealth, rumSourceSummary, type ValidationState } from './receipt';
import { GuideSection } from './Section';
import { StepBlock } from './StepBlock';
import { Verification } from './Verification';

const SIGNAL_TONE: Record<Signal, PillTone> = {
  logs: 'orange',
  metrics: 'blue',
  traces: 'green',
  profiles: 'purple',
};

/**
 * How to connect one source, in the three things there are to do: take the
 * endpoint and token, set the source up, and see its data arrive.
 */
export function Guide({
  source,
  signal,
  streamName,
  overview,
  refreshOverview,
}: {
  source: Source;
  /** The signal the reader came for, when a link named one. */
  signal: Signal | undefined;
  streamName: string;
  overview: homeApi.HomeOverview | undefined;
  refreshOverview: () => Promise<homeApi.HomeOverview>;
}) {
  const { t } = useTranslation('onboarding');
  const text = useSourceText();
  const [guideParams] = useSearchParams();
  const isRum = source.rumPlatform != null;
  const initialApplicationId = isRum ? (guideParams.get('app')?.trim() ?? '') : '';
  const [applicationId, setApplicationId] = React.useState(initialApplicationId);
  const [configuredApplicationId, setConfiguredApplicationId] =
    React.useState(initialApplicationId);
  const context = useIntakeContext({ isRum, applicationId: configuredApplicationId });
  const [validation, setValidation] = React.useState<ValidationState>({ kind: 'idle' });

  const prepared =
    context.applicationValid && !context.tokenLoading && !!context.token && !context.tokenError;
  const verified = validation.kind === 'success';
  const tried = validation.kind === 'warning' || validation.kind === 'error';

  const steps = source.steps.map((step, index) => ({
    step,
    // A sentence may name the endpoint; the token it leaves to the field above.
    text: text.step(source, index, { ENDPOINT: context.endpoint }),
  }));
  const setup = steps.filter(({ step }) => !step.verification);
  const checks = steps.filter(({ step }) => step.verification);
  const Icon = sourceIcon(source);

  const runValidation = async () => {
    setValidation({ kind: 'pending' });

    try {
      const [health, summary] = await Promise.all([
        probeHealth(),
        isRum
          ? rumIntakeApi
              .recentErrorSummary({
                orgId: context.orgId,
                applicationId: context.applicationId,
              })
              .then(rumSourceSummary)
          : refreshOverview().then((next) => summarizeSourceSignals(source, next)),
      ]);

      if (!health.ok) {
        setValidation({
          kind: 'error',
          message: health.message ?? t('datasource_page.validation_endpoint_failed'),
        });
        return;
      }

      const arrived = summary.rows > 0 && summary.lastReceivedAtMicros != null;
      setValidation({
        kind: arrived && summary.status === 'healthy' ? 'success' : 'warning',
        health,
        summary,
      });
    } catch (error) {
      setValidation({ kind: 'error', message: toApiError(error).message });
    }
  };

  return (
    <article
      data-datasource-guide={source.id}
      // The guide of another source arrives rather than swaps in.
      className="ms-rise w-full max-w-[1040px] px-[32px] pb-[40px] pt-[24px]"
    >
      <header className="flex items-start gap-[14px]">
        <span
          aria-hidden
          className="grid h-[40px] w-[40px] shrink-0 place-items-center rounded-lg bg-indigo-dim text-indigo-soft"
        >
          <Icon className="h-5 w-5" strokeWidth={1.8} />
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-x-[10px] gap-y-1">
            <h2 className="m-0 font-sans text-xl font-display-strong tracking-tight text-tx-0">
              {text.name(source)}
            </h2>
            {source.signals.map((item) => (
              <Pill key={item} tone={SIGNAL_TONE[item]}>
                {t(`datasource_page.signals.${item}`)}
              </Pill>
            ))}
          </div>
          <p className="m-0 mt-1 max-w-[680px] font-sans text-md leading-relaxed text-tx-2">
            {text.description(source)}
          </p>
        </div>
        {source.docsUrl && (
          <a
            href={source.docsUrl}
            target="_blank"
            rel="noreferrer"
            className="mt-[6px] inline-flex shrink-0 items-center gap-1 rounded font-sans text-sm font-strong text-tx-2 outline-none transition-colors hover:text-tx-0 focus-visible:text-tx-0 focus-visible:underline"
          >
            {t('datasource_page.docs')}
            <ArrowUpRight aria-hidden className="h-3.5 w-3.5" />
          </a>
        )}
      </header>

      {/* As far under the head as the head is under the panel's edge: the first step's mark
          stands a few pixels inside its own row. */}
      <ol className="m-0 mt-[20px] list-none p-0">
        <GuideSection
          number={1}
          state={prepared ? 'done' : 'current'}
          title={t('datasource_page.steps.prepare.title')}
          description={t('datasource_page.steps.prepare.description')}
        >
          <ConnectionDetails
            endpoints={guideEndpoints(source, signal, streamName, context.endpoint)}
            context={context}
            application={
              isRum ? (
                <ApplicationField
                  value={applicationId}
                  valid={isValidRumApplicationId(applicationId)}
                  confirmed={
                    isValidRumApplicationId(applicationId) &&
                    configuredApplicationId !== '' &&
                    configuredApplicationId === applicationId.trim()
                  }
                  onChange={(value) => {
                    setApplicationId(value);
                    setConfiguredApplicationId('');
                    setValidation({ kind: 'idle' });
                  }}
                  onConfirm={() => setConfiguredApplicationId(applicationId.trim())}
                />
              ) : undefined
            }
          />
        </GuideSection>

        <GuideSection
          number={2}
          state={verified ? 'done' : prepared ? 'current' : 'upcoming'}
          title={t('datasource_page.steps.deploy.title')}
        >
          <div className="space-y-[22px]">
            {setup.map(({ step, text: stepText }, index) => (
              <StepBlock
                key={index}
                text={stepText}
                code={
                  step.code && {
                    language: step.code.lang,
                    content: substitute(step.code.content, context),
                  }
                }
              />
            ))}
          </div>
        </GuideSection>

        <GuideSection
          number={3}
          state={verified ? 'done' : tried ? 'attention' : 'upcoming'}
          title={t('datasource_page.steps.verify.title')}
        >
          <Verification
            source={source}
            validation={validation}
            passiveSummary={isRum ? rumSourceSummary() : summarizeSourceSignals(source, overview)}
            context={context}
            viewPath={viewPathForSource(source)}
            onValidate={runValidation}
          >
            {checks.length > 0 && (
              <div data-guide-checks className="space-y-[22px]">
                {checks.map(({ step, text: stepText }, index) => (
                  <StepBlock
                    key={index}
                    text={stepText}
                    code={
                      step.code && {
                        language: step.code.lang,
                        content: substitute(step.code.content, context),
                      }
                    }
                  />
                ))}
              </div>
            )}
          </Verification>
        </GuideSection>
      </ol>
    </article>
  );
}

/**
 * The addresses to send to. A link that names a signal gets that signal's
 * endpoint; otherwise every signal the source sends has its own line, so
 * nothing in the catalogue has to be set to see the right one.
 */
function guideEndpoints(
  source: Source,
  signal: Signal | undefined,
  streamName: string,
  origin: string,
): GuideEndpoint[] {
  if (source.rumPlatform) return [{ signal: null, url: `${origin}/api/v1/rum/errors` }];

  const shown = signal && source.signals.includes(signal) ? [signal] : source.signals;

  return shown.map((item) => ({
    signal: source.signals.length > 1 ? item : null,
    url: origin + intakePathForSignal(item, streamName),
  }));
}

function viewPathForSource(source: Source): string {
  if (source.rumPlatform) return '/rum/errors';
  if (source.signals.includes('logs')) return '/logs';
  if (source.signals.includes('metrics')) return '/metrics';
  if (source.signals.includes('traces')) return '/traces';
  if (source.signals.includes('profiles')) return '/profiles';
  return '/streams';
}
